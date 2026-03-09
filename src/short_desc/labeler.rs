use std::collections::{HashMap, HashSet};

use crate::desc_options::DescOptions;
use crate::wikidata::WikiData;

use super::word_helpers::{wiki_urlencode, Add2DescArgs};
use super::ShortDescription;

impl ShortDescription {
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

        let mut seen: HashSet<String> = HashSet::new();
        let mut ids: Vec<String> = Vec::new();
        let mut qid_to_prop: HashMap<String, u64> = HashMap::new();
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
            qid_to_prop.insert(qid.clone(), *p);
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

            let label = {
                let preferred = item.get_label(Some(use_lang));
                if preferred != item.get_id() {
                    preferred
                } else {
                    let fallback = item.get_label(None);
                    if fallback == item.get_id() {
                        continue;
                    }
                    fallback
                }
            };

            let p: u64 = qid_to_prop.get(q_str).copied().unwrap_or(0);

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
    pub(super) fn add2desc(
        &self,
        h: &mut Vec<String>,
        item_labels: &HashMap<u64, Vec<String>>,
        args: Add2DescArgs<'_>,
        lang: &str,
    ) {
        let mut h2: Vec<String> = Vec::new();
        for prop in args.props {
            if let Some(labels) = item_labels.get(prop) {
                h2.extend(labels.clone());
            }
        }

        if h2.is_empty() {
            return;
        }

        if let Some(pfx) = args.prefix
            && !h.is_empty() {
                let last = h.len() - 1;
                h[last].push_str(pfx);
            }

        let s = self.list_words(&h2, args.hints, lang);
        if let Some(key) = args.txt_key {
            if lang == "te" {
                h.push(format!("{} {}", s, self.txt(key, lang)));
            } else {
                h.push(format!("{} {}", self.txt(key, lang), s));
            }
        } else {
            h.push(s);
        }
    }

    /// Return the demonym for `country_label` in `lang`.
    /// Tries P1549 on the Wikidata country item first; falls back to the
    /// hardcoded nationality table (`txt2`).
    pub fn get_nationality_from_country(
        &self,
        country_label: &str,
        country_q: Option<&str>,
        lang: &str,
        wd: &WikiData,
    ) -> String {
        if let Some(q) = country_q
            && let Some(demonym) = wd.get_item(q).and_then(|item| item.get_demonym(lang)) {
                return demonym;
            }
        self.txt2(country_label, "nationality", lang)
    }
}
