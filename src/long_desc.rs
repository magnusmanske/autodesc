use std::collections::HashMap;
use std::sync::OnceLock;

use regex::Regex;
use serde_json::Value;

use crate::desc_options::DescOptions;
use crate::short_desc::ShortDescription;
use crate::wikidata::{WikiData, WikiDataItem};

mod lang_en;
mod lang_fr;
mod lang_nl;

/// Supported long-description languages.
const LONG_DESC_LANGUAGES: &[&str] = &["en", "nl", "fr"];

/// A fragment in the long description output.
/// Either a literal string or a reference to a Wikidata item that will be resolved to a label/link.
#[derive(Debug, Clone)]
enum Fragment {
    Text(String),
    Item { q: String, before: String, after: String },
}

/// Date extracted from a Wikidata time claim or qualifier.
#[derive(Debug, Clone)]
struct WdDate {
    time: String,
    precision: u64,
}

/// An item with optional date qualifiers (start/end).
#[derive(Debug, Clone)]
struct DatedItem {
    q: String,
    date_from: Option<WdDate>,
    date_to: Option<WdDate>,
    qualifier_items: HashMap<String, Vec<String>>,
}

/// Language-specific text configuration for person descriptions.
struct LangConfig {
    month_labels: [&'static str; 13],
    pronoun_subject_male: &'static str,
    pronoun_possessive_male: &'static str,
    pronoun_subject_female: &'static str,
    pronoun_possessive_female: &'static str,
    pronoun_subject_neutral: &'static str,
    pronoun_possessive_neutral: &'static str,
    be_present_singular: &'static str,
    be_past_singular: &'static str,
    be_present_neutral: &'static str,
    be_past_neutral: &'static str,
}

/// Runtime state for generating a long description.
struct LongDescState {
    lang: String,
    is_male: bool,
    is_female: bool,
    is_dead: bool,
    fragments: Vec<Fragment>,
    newline: String,
}

impl LongDescState {
    fn pronoun_subject(&self, cfg: &LangConfig) -> &'static str {
        if self.is_male {
            cfg.pronoun_subject_male
        } else if self.is_female {
            cfg.pronoun_subject_female
        } else {
            cfg.pronoun_subject_neutral
        }
    }

    fn pronoun_possessive(&self, cfg: &LangConfig) -> &'static str {
        if self.is_male {
            cfg.pronoun_possessive_male
        } else if self.is_female {
            cfg.pronoun_possessive_female
        } else {
            cfg.pronoun_possessive_neutral
        }
    }

    fn be_present(&self, cfg: &LangConfig) -> &'static str {
        if !self.is_male && !self.is_female {
            cfg.be_present_neutral
        } else {
            cfg.be_present_singular
        }
    }

    fn be_past(&self, cfg: &LangConfig) -> &'static str {
        if !self.is_male && !self.is_female {
            cfg.be_past_neutral
        } else {
            cfg.be_past_singular
        }
    }

    fn push_text(&mut self, s: &str) {
        self.fragments.push(Fragment::Text(s.to_string()));
    }

    fn push_item(&mut self, q: &str, before: &str, after: &str) {
        self.fragments.push(Fragment::Item {
            q: q.to_string(),
            before: before.to_string(),
            after: after.to_string(),
        });
    }

    fn push_newline(&mut self) {
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

/// Generate a long description for a person.
/// Returns `None` if the language is not supported or the item is not a person.
pub async fn generate_long_description(
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

    let mut state = LongDescState {
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
    add_claim_items(claims, "P27", &mut items_to_load); // Country of citizenship
    add_claim_items(claims, "P106", &mut items_to_load); // Occupation
    add_claim_items(claims, "P793", &mut items_to_load); // Significant event
    add_claim_items(claims, "P19", &mut items_to_load); // Birth place
    add_claim_items(claims, "P22", &mut items_to_load); // Father
    add_claim_items(claims, "P25", &mut items_to_load); // Mother
    add_claim_items(claims, "P69", &mut items_to_load); // Educated at
    add_claim_items(claims, "P136", &mut items_to_load); // Genre
    add_claim_items(claims, "P101", &mut items_to_load); // Field of work
    add_claim_items(claims, "P39", &mut items_to_load); // Position held
    add_claim_items(claims, "P463", &mut items_to_load); // Member of
    add_claim_items(claims, "P108", &mut items_to_load); // Employer
    add_claim_items(claims, "P26", &mut items_to_load); // Spouse
    add_claim_items(claims, "P40", &mut items_to_load); // Child
    add_claim_items(claims, "P20", &mut items_to_load); // Death place
    add_claim_items(claims, "P509", &mut items_to_load); // Cause of death
    add_claim_items(claims, "P157", &mut items_to_load); // Killed by
    add_claim_items(claims, "P119", &mut items_to_load); // Place of burial

    // Also load qualifier items (P642 = "of" qualifier on positions, P794 = "as" on employers)
    add_qualifier_items(claims, "P39", "P642", &mut items_to_load);
    add_qualifier_items(claims, "P108", "P794", &mut items_to_load);

    // Batch-load all items
    if let Err(e) = wd.get_item_batch(&items_to_load).await {
        tracing::warn!("Long desc: failed to load items: {}", e);
    }

    // Build the description using the language-specific generator
    match opt.lang.as_str() {
        "en" => lang_en::generate(&mut state, q, claims, sd, wd),
        "nl" => lang_nl::generate(&mut state, q, claims, sd, wd),
        "fr" => lang_fr::generate(&mut state, q, claims, sd, wd),
        _ => return None,
    }

    // Resolve fragments to output string
    let result = resolve_fragments(&state, opt, wd);
    Some(clean_output(&result))
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
            if let Some(page) = wd
                .get_item(q)
                .and_then(|item| {
                    item.raw
                        .get("sitelinks")
                        .and_then(|s| s.get(&wiki))
                        .and_then(|s| s.get("title"))
                        .and_then(|t| t.as_str())
                })
            {
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
            if let Some(page) = wd
                .get_item(q)
                .and_then(|item| {
                    item.raw
                        .get("sitelinks")
                        .and_then(|s| s.get(&wiki))
                        .and_then(|s| s.get("title"))
                        .and_then(|t| t.as_str())
                })
            {
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
fn has_claims(claims: &Value, prop: &str) -> bool {
    claims
        .get(prop)
        .and_then(|v| v.as_array())
        .map(|a| !a.is_empty())
        .unwrap_or(false)
}

/// Add all item Q-ids from claims for a property to the list.
fn add_claim_items(claims: &Value, prop: &str, items: &mut Vec<String>) {
    let arr = match claims.get(prop).and_then(|v| v.as_array()) {
        Some(a) => a,
        None => return,
    };
    for claim in arr {
        if let Some(q) = WikiDataItem::get_claim_target_item_id(claim) {
            if !items.contains(&q) {
                items.push(q);
            }
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
        if let Some(qualifiers) = claim.get("qualifiers") {
            if let Some(qual_arr) = qualifiers.get(qual_prop).and_then(|v| v.as_array()) {
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
}

/// Get the first claim target item Q-id for a property.
fn get_first_claim_item(claims: &Value, prop: &str) -> Option<String> {
    let arr = claims.get(prop)?.as_array()?;
    arr.first()
        .and_then(WikiDataItem::get_claim_target_item_id)
}

/// Get all claim target item Q-ids for a property.
fn get_claim_item_ids(claims: &Value, prop: &str) -> Vec<String> {
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
fn get_dated_items(claims: &Value, prop: &str, qualifier_keys: &[&str]) -> Vec<DatedItem> {
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

        let date_from = qualifiers
            .and_then(|qs| {
                // Try P581 (point in time) first, then P580 (start time)
                extract_date_qualifier(qs, "P581")
                    .or_else(|| extract_date_qualifier(qs, "P580"))
            });

        let date_to = qualifiers
            .and_then(|qs| {
                // Try P581 first, then P582 (end time)
                extract_date_qualifier(qs, "P581")
                    .or_else(|| extract_date_qualifier(qs, "P582"))
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
fn extract_date_qualifier(qualifiers: &Value, prop: &str) -> Option<WdDate> {
    let arr = qualifiers.get(prop)?.as_array()?;
    let first = arr.first()?;
    let dv = first.get("datavalue")?.get("value")?;
    let time = dv.get("time")?.as_str()?.to_string();
    let precision = dv.get("precision")?.as_u64().unwrap_or(9);
    Some(WdDate { time, precision })
}

/// Extract a WdDate from a claim's mainsnak time value.
fn extract_claim_date(claim: &Value) -> Option<WdDate> {
    let value = claim
        .get("mainsnak")?
        .get("datavalue")?
        .get("value")?;
    let time = value.get("time")?.as_str()?.to_string();
    let precision = value.get("precision")?.as_u64().unwrap_or(9);
    Some(WdDate { time, precision })
}

/// Get the claim target string value (e.g. for birth name P513).
fn get_first_claim_string(claims: &Value, prop: &str) -> Option<String> {
    let arr = claims.get(prop)?.as_array()?;
    WikiDataItem::get_claim_target_string(arr.first()?)
}

/// Get the main label for the item.
fn get_main_title_label(q: &str, wd: &WikiData, lang: &str) -> String {
    wd.get_item(q)
        .map(|item| item.get_label(Some(lang)))
        .unwrap_or_else(|| q.to_string())
}

/// Format the main title with bold markup.
fn push_bold_title(state: &mut LongDescState, label: &str, opt: &DescOptions) {
    match opt.links.as_str() {
        "text" => {
            state.push_text(label);
            state.push_text(" ");
        }
        "wiki" => {
            state.push_text("'''");
            state.push_text(label);
            state.push_text("''' ");
        }
        _ => {
            state.push_text("<b>");
            state.push_text(label);
            state.push_text("</b> ");
        }
    }
}

/// Get a separator for list items (English-style: "a, b, and c").
fn get_sep_after_en(len: usize, pos: usize) -> &'static str {
    if pos + 1 == len {
        " "
    } else if pos == 0 && len == 2 {
        " and "
    } else if len == pos + 2 {
        ", and "
    } else {
        ", "
    }
}

/// Get a separator for list items (Dutch-style: "a, b, en c").
fn get_sep_after_nl(len: usize, pos: usize) -> &'static str {
    if pos + 1 == len {
        " "
    } else if pos == 0 && len == 2 {
        " en "
    } else if len == pos + 2 {
        ", en "
    } else {
        ", "
    }
}

/// Get a separator for list items (French-style: "a, b et c").
fn get_sep_after_fr(len: usize, pos: usize) -> &'static str {
    if pos + 1 == len {
        " "
    } else if pos == 0 && len == 2 {
        " et "
    } else if len == pos + 2 {
        " et "
    } else {
        ", "
    }
}

/// Render a simple list of items: prefix + item1, item2, ... + suffix.
fn push_simple_list(state: &mut LongDescState, items: &[String], start: &str, end: &str, sep_fn: fn(usize, usize) -> &'static str) {
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
fn parse_time(time: &str) -> Option<(i32, &str, u32, u32)> {
    let re = time_regex();
    let caps = re.captures(time)?;
    let sign = if caps.get(1)?.as_str() == "+" { 1 } else { -1 };
    let year_str = caps.get(2)?.as_str();
    let year: i32 = year_str.parse::<i32>().ok()? * sign;
    let month: u32 = caps.get(3)?.as_str().parse().ok()?;
    let day: u32 = caps.get(4)?.as_str().parse().ok()?;
    // Return absolute year as string for display
    Some((year, year_str, month, day))
}
