use std::collections::HashMap;

use crate::desc_options::DescOptions;
use crate::wikidata::{sanitize_q, WikiData};

mod claims;
mod describers;
mod labeler;
mod word_helpers;

pub use word_helpers::WordHints;

pub struct ShortDescription {
    pub stock: HashMap<String, HashMap<String, String>>,
    pub language_specific: HashMap<String, HashMap<String, HashMap<String, String>>>,
}

impl ShortDescription {
    pub fn new() -> Self {
        let stock_json = include_str!("../data/stock.json");
        let stock: HashMap<String, HashMap<String, String>> =
            serde_json::from_str(stock_json).unwrap_or_default();

        Self {
            stock,
            language_specific: HashMap::new(),
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
}

impl Default for ShortDescription {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::word_helpers::{clean_spaces, split_link, uc_first};
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
        let empty = WordHints::default();
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
        assert_eq!(uc_first("hello"), "Hello");
        assert_eq!(uc_first(""), "");
        assert_eq!(uc_first("Hello"), "Hello");
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
    fn test_has_pq_newer_id_format() {
        let claims = serde_json::json!({
            "P31": [
                {
                    "mainsnak": {
                        "datavalue": {
                            "value": {
                                "entity-type": "item",
                                "id": "Q5"
                            }
                        }
                    }
                }
            ]
        });
        assert!(ShortDescription::has_pq(&claims, 31, 5));
        assert!(!ShortDescription::has_pq(&claims, 31, 42));
    }

    #[test]
    fn test_has_pq_missing_mainsnak_continues() {
        let claims = serde_json::json!({
            "P31": [
                {},
                {
                    "mainsnak": {
                        "datavalue": {
                            "value": { "numeric-id": 5 }
                        }
                    }
                }
            ]
        });
        assert!(ShortDescription::has_pq(&claims, 31, 5));
    }

    #[test]
    fn test_add_items_from_claims_uses_continue_not_return() {
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
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].1, "Q36180");
    }

    #[test]
    fn test_add_items_from_claims_numeric_id_fallback() {
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
        assert_eq!(clean_spaces("hello  world"), "hello world");
        assert_eq!(clean_spaces("foo ,bar"), "foo,bar");
        assert_eq!(clean_spaces("  trim  "), "trim");
    }

    #[test]
    fn test_split_link() {
        let (_, before, inner, after) = split_link("<a href='test'>Hello</a>");
        assert_eq!(before, "<a href='test'>");
        assert_eq!(inner, "Hello");
        assert_eq!(after, "</a>");

        let (_, before, inner, after) = split_link("plain text");
        assert_eq!(before, "");
        assert_eq!(inner, "plain text");
        assert_eq!(after, "");

        let (_, before, inner, after) = split_link("[[Page|Label]]");
        assert_eq!(before, "[[Page|");
        assert_eq!(inner, "Label");
        assert_eq!(after, "]]");
    }

    #[test]
    fn test_modify_word_gender() {
        let sd = ShortDescription::new();
        let female = WordHints {
            is_female: true,
            ..Default::default()
        };
        let male = WordHints {
            is_male: true,
            ..Default::default()
        };
        assert_eq!(sd.modify_word("actor", &female, "en"), "actress");
        assert_eq!(sd.modify_word("actor", &male, "en"), "actor");
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
        let (q, desc) = sd.load_item("Q42", &mut opt, &mut wd).await;
        assert_eq!(q, "Q42");
        assert!(!desc.is_empty());
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
        assert!(
            desc.contains("[[") || !desc.is_empty(),
            "Wiki link mode should produce wikitext or plain labels, got: {}",
            desc
        );
    }
}
