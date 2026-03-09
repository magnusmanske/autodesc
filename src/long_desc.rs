use std::collections::HashMap;
use std::sync::OnceLock;

use regex::Regex;
use serde_json::Value;

use crate::desc_options::DescOptions;
use crate::short_desc::ShortDescription;
use crate::wikidata::{WikiData, WikiDataItem};

mod lang_de;
mod lang_en;
mod lang_fr;
mod lang_nl;

/// Supported long-description languages.
const LONG_DESC_LANGUAGES: &[&str] = &["de", "en", "nl", "fr"];

/// A fragment in the long description output.
/// Either a literal string or a reference to a Wikidata item that will be resolved to a label/link.
#[derive(Debug, Clone)]
pub(super) enum Fragment {
    Text(String),
    Item {
        q: String,
        before: String,
        after: String,
    },
}

/// Date extracted from a Wikidata time claim or qualifier.
#[derive(Debug, Clone)]
pub(super) struct WdDate {
    pub(super) time: String,
    pub(super) precision: u64,
}

/// An item with optional date qualifiers (start/end).
#[derive(Debug, Clone)]
pub(super) struct DatedItem {
    pub(super) q: String,
    pub(super) date_from: Option<WdDate>,
    pub(super) date_to: Option<WdDate>,
    pub(super) qualifier_items: HashMap<String, Vec<String>>,
}

/// Language-specific text configuration for person descriptions.
pub(super) struct LangConfig {
    pub(super) month_labels: [&'static str; 13],
    pub(super) pronoun_subject_male: &'static str,
    pub(super) pronoun_possessive_male: &'static str,
    pub(super) pronoun_subject_female: &'static str,
    pub(super) pronoun_possessive_female: &'static str,
    pub(super) pronoun_subject_neutral: &'static str,
    pub(super) pronoun_possessive_neutral: &'static str,
    pub(super) be_present_singular: &'static str,
    pub(super) be_past_singular: &'static str,
    pub(super) be_present_neutral: &'static str,
    pub(super) be_past_neutral: &'static str,
}

/// Runtime state for generating a long description.
pub(super) struct LongDescState {
    pub(super) lang: String,
    pub(super) is_male: bool,
    pub(super) is_female: bool,
    pub(super) is_dead: bool,
    pub(super) fragments: Vec<Fragment>,
    pub(super) newline: String,
}

impl LongDescState {
    pub(super) fn pronoun_subject(&self, cfg: &LangConfig) -> &'static str {
        if self.is_male {
            cfg.pronoun_subject_male
        } else if self.is_female {
            cfg.pronoun_subject_female
        } else {
            cfg.pronoun_subject_neutral
        }
    }

    pub(super) fn pronoun_possessive(&self, cfg: &LangConfig) -> &'static str {
        if self.is_male {
            cfg.pronoun_possessive_male
        } else if self.is_female {
            cfg.pronoun_possessive_female
        } else {
            cfg.pronoun_possessive_neutral
        }
    }

    pub(super) fn be_present(&self, cfg: &LangConfig) -> &'static str {
        if !self.is_male && !self.is_female {
            cfg.be_present_neutral
        } else {
            cfg.be_present_singular
        }
    }

    pub(super) fn be_past(&self, cfg: &LangConfig) -> &'static str {
        if !self.is_male && !self.is_female {
            cfg.be_past_neutral
        } else {
            cfg.be_past_singular
        }
    }

    pub(super) fn push_text(&mut self, s: &str) {
        self.fragments.push(Fragment::Text(s.to_string()));
    }

    pub(super) fn push_item(&mut self, q: &str, before: &str, after: &str) {
        self.fragments.push(Fragment::Item {
            q: q.to_string(),
            before: before.to_string(),
            after: after.to_string(),
        });
    }

    pub(super) fn push_newline(&mut self) {
        self.fragments.push(Fragment::Text(self.newline.clone()));
    }
}

fn time_regex() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"^([+-])0*(\d+)-(\d{2})-(\d{2})").expect("regex is valid"))
}

fn clean_output_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r" +").expect("regex is valid"))
}

/// Check if a long description is available for the given language.
pub fn is_long_desc_available(lang: &str) -> bool {
    LONG_DESC_LANGUAGES.contains(&lang)
}

/// Trait implemented by each supported language's generator.
/// Provides language-specific text for each section of the long description.
/// The default `generate` method calls the five sections in order.
pub(super) trait LangGenerator {
    fn add_first_sentence(
        &self,
        state: &mut LongDescState,
        q: &str,
        claims: &Value,
        sd: &ShortDescription,
        wd: &WikiData,
    );
    fn add_birth_text(&self, state: &mut LongDescState, claims: &Value);
    fn add_work_text(&self, state: &mut LongDescState, claims: &Value);
    fn add_family_text(&self, state: &mut LongDescState, claims: &Value);
    fn add_death_text(&self, state: &mut LongDescState, claims: &Value);

    /// Generate the full description by calling the five sections in order.
    fn generate(
        &self,
        state: &mut LongDescState,
        q: &str,
        claims: &Value,
        sd: &ShortDescription,
        wd: &WikiData,
    ) {
        self.add_first_sentence(state, q, claims, sd, wd);
        self.add_birth_text(state, claims);
        self.add_work_text(state, claims);
        self.add_family_text(state, claims);
        self.add_death_text(state, claims);
    }
}

/// Orchestrates long description generation for any supported language.
pub struct LongDescGenerator;

impl LongDescGenerator {
    /// Generate a long description for a person.
    /// Returns `None` if the language is not supported or the item is not a person.
    pub async fn generate(
        sd: &ShortDescription,
        q: &str,
        claims: &Value,
        opt: &DescOptions,
        wd: &mut WikiData,
    ) -> Option<String> {
        if !is_long_desc_available(&opt.lang) {
            return None;
        }

        // Only persons get long descriptions
        if !ShortDescription::is_person_public(claims) {
            return None;
        }

        let claims_clone = claims.clone();
        let opt_clone = opt.clone();
        let (state, items_to_load) =
            tokio::task::spawn_blocking(move || Self::get_items_to_load(&claims_clone, &opt_clone))
                .await
                .ok()?;

        // Batch-load all items
        if let Err(e) = wd.get_item_batch(&items_to_load).await {
            tracing::warn!("Long desc: failed to load items: {}", e);
        }

        tokio::task::block_in_place(|| Self::finalize_generation(sd, q, claims, opt, wd, state))
    }

    fn get_items_to_load(claims: &Value, opt: &DescOptions) -> (LongDescState, Vec<String>) {
        let is_male = ShortDescription::has_pq_public(claims, 21, 6581097)
            || ShortDescription::has_pq_public(claims, 21, 2449503);
        let is_female = ShortDescription::has_pq_public(claims, 21, 6581072)
            || ShortDescription::has_pq_public(claims, 21, 1052281);
        let is_dead = has_claims(claims, "P570");

        let newline = match opt.links.as_str() {
            "text" => "\n".to_string(),
            "wiki" => "\n\n".to_string(),
            _ => "<br/>".to_string(),
        };

        let state = LongDescState {
            lang: opt.lang.clone(),
            is_male,
            is_female,
            is_dead,
            fragments: Vec::new(),
            newline,
        };

        // Collect all Q-ids we need to load
        let mut items_to_load: Vec<String> = Vec::new();

        // Items needed for description sections
        add_claim_items(claims, "P27", &mut items_to_load);
        // Country of citizenship
        add_claim_items(claims, "P106", &mut items_to_load);
        // Occupation
        add_claim_items(claims, "P793", &mut items_to_load);
        // Significant event
        add_claim_items(claims, "P19", &mut items_to_load);
        // Birth place
        add_claim_items(claims, "P22", &mut items_to_load);
        // Father
        add_claim_items(claims, "P25", &mut items_to_load);
        // Mother
        add_claim_items(claims, "P69", &mut items_to_load);
        // Educated at
        add_claim_items(claims, "P136", &mut items_to_load);
        // Genre
        add_claim_items(claims, "P101", &mut items_to_load);
        // Field of work
        add_claim_items(claims, "P39", &mut items_to_load);
        // Position held
        add_claim_items(claims, "P463", &mut items_to_load);
        // Member of
        add_claim_items(claims, "P108", &mut items_to_load);
        // Employer
        add_claim_items(claims, "P26", &mut items_to_load);
        // Spouse
        add_claim_items(claims, "P40", &mut items_to_load);
        // Child
        add_claim_items(claims, "P20", &mut items_to_load);
        // Death place
        add_claim_items(claims, "P509", &mut items_to_load);
        // Cause of death
        add_claim_items(claims, "P157", &mut items_to_load);
        // Killed by
        add_claim_items(claims, "P119", &mut items_to_load);
        // Place of burial
        add_claim_items(claims, "P800", &mut items_to_load);
        // Notable work

        // Also load qualifier items (P642 = "of" qualifier on positions, P794 = "as" on employers)
        add_qualifier_items(claims, "P39", "P642", &mut items_to_load);
        add_qualifier_items(claims, "P108", "P794", &mut items_to_load);
        (state, items_to_load)
    }

    fn finalize_generation(
        sd: &ShortDescription,
        q: &str,
        claims: &Value,
        opt: &DescOptions,
        wd: &mut WikiData,
        mut state: LongDescState,
    ) -> Option<String> {
        // Dispatch to the language-specific generator
        let lang_gen: &dyn LangGenerator = match opt.lang.as_str() {
            "de" => &lang_de::LangDe,
            "en" => &lang_en::LangEn,
            "nl" => &lang_nl::LangNl,
            "fr" => &lang_fr::LangFr,
            _ => return None,
        };
        lang_gen.generate(&mut state, q, claims, sd, wd);

        // Resolve fragments to output string
        let result = resolve_fragments(&state, opt, wd);
        Some(clean_output(&result))
    }
}

/// Resolve all fragments into the final output string.
fn resolve_fragments(state: &LongDescState, opt: &DescOptions, wd: &WikiData) -> String {
    let mut output = String::new();
    for frag in &state.fragments {
        match frag {
            Fragment::Text(s) => output.push_str(s),
            Fragment::Item { q, before, after } => {
                output.push_str(before);
                let label = wd
                    .get_item(q)
                    .map(|item| {
                        let label = item.get_label(Some(&state.lang));
                        if label == item.get_id() {
                            item.get_label(None)
                        } else {
                            label
                        }
                    })
                    .unwrap_or_else(|| q.clone());
                let formatted = format_link(q, &label, opt, wd);
                output.push_str(&formatted);
                output.push_str(after);
            }
        }
    }
    output
}

/// Format a Q-id as a link according to the link mode.
fn format_link(q: &str, label: &str, opt: &DescOptions, wd: &WikiData) -> String {
    let linktarget = if !opt.linktarget.is_empty() {
        format!(" target='{}'", opt.linktarget)
    } else {
        String::new()
    };
    let wiki = format!("{}wiki", opt.lang);

    match opt.links.as_str() {
        "text" | "" => label.to_string(),
        "wikidata" => {
            format!(
                "<a href='https://www.wikidata.org/wiki/{q}'{lt}>{label}</a>",
                q = q,
                lt = linktarget,
                label = label
            )
        }
        "wiki" => {
            if let Some(page) = wd.get_item(q).and_then(|item| {
                item.raw
                    .get("sitelinks")
                    .and_then(|s| s.get(&wiki))
                    .and_then(|s| s.get("title"))
                    .and_then(|t| t.as_str())
            }) {
                if page == label {
                    format!("[[{}]]", label)
                } else {
                    format!("[[{}|{}]]", page, label)
                }
            } else {
                label.to_string()
            }
        }
        "wikipedia" => {
            if let Some(page) = wd.get_item(q).and_then(|item| {
                item.raw
                    .get("sitelinks")
                    .and_then(|s| s.get(&wiki))
                    .and_then(|s| s.get("title"))
                    .and_then(|t| t.as_str())
            }) {
                let encoded = urlencoding::encode(&page.replace(' ', "_")).to_string();
                format!(
                    "<a href='https://{lang}.wikipedia.org/wiki/{page}'{lt}>{label}</a>",
                    lang = opt.lang,
                    page = encoded,
                    lt = linktarget,
                    label = label
                )
            } else {
                label.to_string()
            }
        }
        "reasonator" => {
            format!(
                "<a href='/reasonator/?lang={lang}&q={q}'{lt}>{label}</a>",
                lang = opt.lang,
                q = q,
                lt = linktarget,
                label = label
            )
        }
        _ => label.to_string(),
    }
}

/// Clean up the output text (excess spaces, space before punctuation, etc.).
fn clean_output(s: &str) -> String {
    let s = clean_output_re().replace_all(s, " ");
    let s = s.replace(" .", ".");
    let s = s.replace(" ,", ",");
    let s = s.replace("..", ".");
    // Collapse multiple <br/> into one
    let s = Regex::new(r"(<br/>\s*)+")
        .map(|re| re.replace_all(&s, "<br/>\n").to_string())
        .unwrap_or(s);
    s.trim().to_string()
}

// ─── Wikidata claim helpers ───────────────────────────────────────────────────

/// Check if claims contain any values for a property.
pub(super) fn has_claims(claims: &Value, prop: &str) -> bool {
    claims
        .get(prop)
        .and_then(|v| v.as_array())
        .map(|a| !a.is_empty())
        .unwrap_or(false)
}

/// Add all item Q-ids from claims for a property to the list.
pub(super) fn add_claim_items(claims: &Value, prop: &str, items: &mut Vec<String>) {
    let arr = match claims.get(prop).and_then(|v| v.as_array()) {
        Some(a) => a,
        None => return,
    };
    for claim in arr {
        if let Some(q) = WikiDataItem::get_claim_target_item_id(claim)
            && !items.contains(&q) {
                items.push(q);
            }
    }
}

/// Add qualifier item Q-ids from claims.
fn add_qualifier_items(claims: &Value, prop: &str, qual_prop: &str, items: &mut Vec<String>) {
    let arr = match claims.get(prop).and_then(|v| v.as_array()) {
        Some(a) => a,
        None => return,
    };
    for claim in arr {
        if let Some(qualifiers) = claim.get("qualifiers")
            && let Some(qual_arr) = qualifiers.get(qual_prop).and_then(|v| v.as_array()) {
                for qual in qual_arr {
                    if let Some(q) = qual
                        .get("datavalue")
                        .and_then(|dv| dv.get("value"))
                        .and_then(|v| v.get("id"))
                        .and_then(|id| id.as_str())
                    {
                        let q = q.to_string();
                        if !items.contains(&q) {
                            items.push(q);
                        }
                    } else if let Some(nid) = qual
                        .get("datavalue")
                        .and_then(|dv| dv.get("value"))
                        .and_then(|v| v.get("numeric-id"))
                        .and_then(|n| n.as_u64())
                    {
                        let q = format!("Q{}", nid);
                        if !items.contains(&q) {
                            items.push(q);
                        }
                    }
                }
            }
    }
}

/// Get the first claim target item Q-id for a property.
pub(super) fn get_first_claim_item(claims: &Value, prop: &str) -> Option<String> {
    let arr = claims.get(prop)?.as_array()?;
    arr.first().and_then(WikiDataItem::get_claim_target_item_id)
}

/// Get all claim target item Q-ids for a property.
pub(super) fn get_claim_item_ids(claims: &Value, prop: &str) -> Vec<String> {
    claims
        .get(prop)
        .and_then(|v| v.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(WikiDataItem::get_claim_target_item_id)
                .collect()
        })
        .unwrap_or_default()
}

/// Extract items with date qualifiers from claims.
pub(super) fn get_dated_items(
    claims: &Value,
    prop: &str,
    qualifier_keys: &[&str],
) -> Vec<DatedItem> {
    let arr = match claims.get(prop).and_then(|v| v.as_array()) {
        Some(a) => a,
        None => return Vec::new(),
    };

    let mut result = Vec::new();
    for claim in arr {
        let q = match WikiDataItem::get_claim_target_item_id(claim) {
            Some(q) => q,
            None => continue,
        };

        let qualifiers = claim.get("qualifiers");

        let date_from = qualifiers.and_then(|qs| {
            // Try P581 (point in time) first, then P580 (start time)
            extract_date_qualifier(qs, "P581").or_else(|| extract_date_qualifier(qs, "P580"))
        });

        let date_to = qualifiers.and_then(|qs| {
            // Try P581 first, then P582 (end time)
            extract_date_qualifier(qs, "P581").or_else(|| extract_date_qualifier(qs, "P582"))
        });

        let mut qualifier_items: HashMap<String, Vec<String>> = HashMap::new();
        if let Some(qs) = qualifiers {
            for key in qualifier_keys {
                if let Some(arr) = qs.get(*key).and_then(|v| v.as_array()) {
                    let items: Vec<String> = arr
                        .iter()
                        .filter_map(|v| {
                            let dv = v.get("datavalue")?.get("value")?;
                            if let Some(id) = dv.get("id").and_then(|i| i.as_str()) {
                                Some(id.to_string())
                            } else {
                                dv.get("numeric-id")
                                    .and_then(|n| n.as_u64())
                                    .map(|n| format!("Q{}", n))
                            }
                        })
                        .collect();
                    if !items.is_empty() {
                        qualifier_items.insert(key.to_string(), items);
                    }
                }
            }
        }

        result.push(DatedItem {
            q,
            date_from,
            date_to,
            qualifier_items,
        });
    }

    // Sort by date
    result.sort_by(|a, b| {
        let a_time = a
            .date_from
            .as_ref()
            .or(a.date_to.as_ref())
            .map(|d| d.time.as_str())
            .unwrap_or("");
        let b_time = b
            .date_from
            .as_ref()
            .or(b.date_to.as_ref())
            .map(|d| d.time.as_str())
            .unwrap_or("");
        a_time.cmp(b_time)
    });

    result
}

/// Extract a date from a qualifier.
pub(super) fn extract_date_qualifier(qualifiers: &Value, prop: &str) -> Option<WdDate> {
    let arr = qualifiers.get(prop)?.as_array()?;
    let first = arr.first()?;
    let dv = first.get("datavalue")?.get("value")?;
    let time = dv.get("time")?.as_str()?.to_string();
    let precision = dv.get("precision")?.as_u64().unwrap_or(9);
    Some(WdDate { time, precision })
}

/// Extract a WdDate from a claim's mainsnak time value.
pub(super) fn extract_claim_date(claim: &Value) -> Option<WdDate> {
    let value = claim.get("mainsnak")?.get("datavalue")?.get("value")?;
    let time = value.get("time")?.as_str()?.to_string();
    let precision = value.get("precision")?.as_u64().unwrap_or(9);
    Some(WdDate { time, precision })
}

/// Get the claim target string value (e.g. for birth name P513).
pub(super) fn get_first_claim_string(claims: &Value, prop: &str) -> Option<String> {
    let arr = claims.get(prop)?.as_array()?;
    WikiDataItem::get_claim_target_string(arr.first()?)
}

/// Get the main label for the item.
pub(super) fn get_main_title_label(q: &str, wd: &WikiData, lang: &str) -> String {
    wd.get_item(q)
        .map(|item| item.get_label(Some(lang)))
        .unwrap_or_else(|| q.to_string())
}

/// Render a simple list of items: prefix + item1, item2, ... + suffix.
pub(super) fn push_simple_list(
    state: &mut LongDescState,
    items: &[String],
    start: &str,
    end: &str,
    sep_fn: fn(usize, usize) -> &'static str,
) {
    if items.is_empty() {
        return;
    }
    state.push_text(start);
    for (k, q) in items.iter().enumerate() {
        state.push_item(q, "", " ");
        state.push_text(sep_fn(items.len(), k));
    }
    state.push_text(end);
}

/// Parse a Wikidata time string into components.
pub(super) fn parse_time(time: &str) -> Option<(i32, &str, u32, u32)> {
    let re = time_regex();
    let caps = re.captures(time)?;
    let sign = if caps.get(1)?.as_str() == "+" { 1 } else { -1 };
    let year_str = caps.get(2)?.as_str();
    let year: i32 = year_str.parse::<i32>().ok()? * sign;
    let month: u32 = caps.get(3)?.as_str().parse().ok()?;
    let day: u32 = caps.get(4)?.as_str().parse().ok()?;
    Some((year, year_str, month, day))
}
