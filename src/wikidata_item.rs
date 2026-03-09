use std::collections::HashMap;

use serde_json::Value;

/// Languages tried in order when no specific language is requested.
pub const MAIN_LANGUAGES: &[&str] = &[
    "en", "de", "fr", "nl", "es", "it", "pl", "pt", "ja", "ru", "hu", "sv", "fi",
];

/// Normalize an entity ID to uppercase with proper prefix (removes whitespace).
pub fn unified_id(name: &str) -> String {
    name.chars()
        .filter(|c| !c.is_whitespace())
        .collect::<String>()
        .to_uppercase()
}

/// Sanitize a Q-id: ensure it starts with "Q".
pub fn sanitize_q(q: &str) -> String {
    let q = q.trim().to_uppercase();
    if q.chars().all(|c| c.is_ascii_digit()) {
        format!("Q{}", q)
    } else {
        q
    }
}

/// Represents a single Wikidata entity with helper methods for extracting data.
#[derive(Debug, Clone)]
pub struct WikiDataItem {
    pub raw: Value,
    placeholder: bool,
}

impl WikiDataItem {
    pub fn new(raw: Value) -> Self {
        let placeholder = raw.is_null();
        Self { raw, placeholder }
    }

    pub fn placeholder() -> Self {
        Self {
            raw: Value::Null,
            placeholder: true,
        }
    }

    pub fn is_placeholder(&self) -> bool {
        self.placeholder
    }

    pub fn is_item(&self) -> bool {
        self.raw.get("ns").and_then(|v| v.as_i64()) == Some(0)
    }

    pub fn get_id(&self) -> String {
        self.raw
            .get("id")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string()
    }

    pub fn get_label(&self, language: Option<&str>) -> String {
        let fallback = self.get_id();

        if let Some(lang) = language {
            if let Some(label) = self
                .raw
                .get("labels")
                .and_then(|ls| ls.get(lang))
                .and_then(|l| l.get("value"))
                .and_then(|v| v.as_str())
            {
                return label.to_string();
            }
            // Try "mul" (multilingual) before giving up
            if let Some(label) = self
                .raw
                .get("labels")
                .and_then(|ls| ls.get("mul"))
                .and_then(|l| l.get("value"))
                .and_then(|v| v.as_str())
            {
                return label.to_string();
            }
            return fallback;
        }

        // No language specified: try main languages in order, then "mul", then any available one.
        for lang in MAIN_LANGUAGES {
            if let Some(label) = self
                .raw
                .get("labels")
                .and_then(|ls| ls.get(*lang))
                .and_then(|l| l.get("value"))
                .and_then(|v| v.as_str())
            {
                return label.to_string();
            }
        }

        if let Some(label) = self
            .raw
            .get("labels")
            .and_then(|ls| ls.get("mul"))
            .and_then(|l| l.get("value"))
            .and_then(|v| v.as_str())
        {
            return label.to_string();
        }

        // Fallback: pick first available language.
        if let Some(labels) = self.raw.get("labels").and_then(|l| l.as_object()) {
            for (_lang, val) in labels {
                if let Some(label) = val.get("value").and_then(|v| v.as_str()) {
                    return label.to_string();
                }
            }
        }

        fallback
    }

    /// Get the gendered form of the label for an occupation item.
    /// Uses P2521 ("female form of label") when `is_female`, P3321 ("male form of label") otherwise.
    /// Both are monolingualtext properties.
    pub fn get_gendered_label(&self, lang: &str, is_female: bool) -> Option<String> {
        let prop = if is_female { "P2521" } else { "P3321" };
        let claims = self.get_claims_for_property(prop);
        for claim in &claims {
            if let Some(value) = claim
                .get("mainsnak")
                .and_then(|s| s.get("datavalue"))
                .and_then(|dv| dv.get("value"))
            {
                let claim_lang = value.get("language").and_then(|l| l.as_str());
                let text = value.get("text").and_then(|t| t.as_str());
                if let (Some(cl), Some(t)) = (claim_lang, text) {
                    if cl == lang {
                        return Some(t.to_string());
                    }
                }
            }
        }
        None
    }

    /// Get the demonym (P1549) for this item in the given language.
    /// P1549 values are monolingualtext, so we look for the one matching `lang`.
    pub fn get_demonym(&self, lang: &str) -> Option<String> {
        let claims = self.get_claims_for_property("P1549");
        for claim in &claims {
            if let Some(value) = claim
                .get("mainsnak")
                .and_then(|s| s.get("datavalue"))
                .and_then(|dv| dv.get("value"))
            {
                let claim_lang = value.get("language").and_then(|l| l.as_str());
                let text = value.get("text").and_then(|t| t.as_str());
                if let (Some(cl), Some(t)) = (claim_lang, text) {
                    if cl == lang {
                        return Some(t.to_string());
                    }
                }
            }
        }
        None
    }

    pub fn get_desc(&self, language: Option<&str>) -> String {
        if let Some(lang) = language {
            return self
                .raw
                .get("descriptions")
                .and_then(|ds| ds.get(lang))
                .and_then(|d| d.get("value"))
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
        }

        for lang in MAIN_LANGUAGES {
            let desc = self
                .raw
                .get("descriptions")
                .and_then(|ds| ds.get(*lang))
                .and_then(|d| d.get("value"))
                .and_then(|v| v.as_str())
                .unwrap_or("");
            if !desc.is_empty() {
                return desc.to_string();
            }
        }
        String::new()
    }

    pub fn get_claims_for_property(&self, p: &str) -> Vec<Value> {
        let p = unified_id(p);
        self.raw
            .get("claims")
            .and_then(|c| c.get(&p))
            .and_then(|v| v.as_array())
            .cloned()
            .unwrap_or_default()
    }

    pub fn has_claims(&self, p: &str) -> bool {
        !self.get_claims_for_property(p).is_empty()
    }

    /// Extract the target item Q-id from a claim's mainsnak.
    /// Returns `None` for non-item entity types (e.g. properties).
    pub fn get_claim_target_item_id(claim: &Value) -> Option<String> {
        let nid = claim.get("mainsnak")?.get("datavalue")?.get("value")?;

        let entity_type = nid.get("entity-type").and_then(|v| v.as_str())?;
        if entity_type != "item" {
            return None;
        }

        // Prefer the "id" string field (newer API format) over "numeric-id".
        if let Some(id) = nid.get("id").and_then(|v| v.as_str()) {
            return Some(id.to_string());
        }
        let numeric_id = nid.get("numeric-id").and_then(|v| v.as_u64())?;
        Some(format!("Q{}", numeric_id))
    }

    pub fn get_claim_target_string(claim: &Value) -> Option<String> {
        Self::get_claim_value_with_type(claim, "string")
            .and_then(|v| v.as_str().map(|s| s.to_string()))
    }

    pub fn get_claim_date(claim: &Value) -> Option<Value> {
        Self::get_claim_value_with_type(claim, "time")
    }

    pub fn get_claim_value_with_type(claim: &Value, dtype: &str) -> Option<Value> {
        let mainsnak = claim.get("mainsnak")?;
        let datavalue = mainsnak.get("datavalue")?;
        let vtype = datavalue.get("type")?.as_str()?;
        if vtype != dtype {
            return None;
        }
        Some(datavalue.get("value")?.clone())
    }

    pub fn has_claim_item_link(&self, p: &str, q: &str) -> bool {
        let q = unified_id(q);
        self.get_claims_for_property(p)
            .iter()
            .filter_map(Self::get_claim_target_item_id)
            .any(|id| id == q)
    }

    pub fn get_claim_items_for_property(&self, p: &str) -> Vec<String> {
        self.get_claims_for_property(p)
            .iter()
            .filter_map(Self::get_claim_target_item_id)
            .collect()
    }

    pub fn get_strings_for_property(&self, p: &str) -> Vec<String> {
        self.get_claims_for_property(p)
            .iter()
            .filter_map(Self::get_claim_target_string)
            .collect()
    }

    pub fn get_wiki_links(&self) -> HashMap<String, Value> {
        self.raw
            .get("sitelinks")
            .and_then(|s| s.as_object())
            .map(|obj| obj.iter().map(|(k, v)| (k.clone(), v.clone())).collect())
            .unwrap_or_default()
    }

    pub fn get_aliases_for_language(&self, lang: &str, include_labels: bool) -> Vec<String> {
        let mut aliases = HashMap::new();

        if let Some(arr) = self
            .raw
            .get("aliases")
            .and_then(|a| a.get(lang))
            .and_then(|a| a.as_array())
        {
            for item in arr {
                if let Some(val) = item.get("value").and_then(|v| v.as_str()) {
                    aliases.insert(val.to_string(), true);
                }
            }
        }

        if include_labels {
            if let Some(label) = self
                .raw
                .get("labels")
                .and_then(|ls| ls.get(lang))
                .and_then(|l| l.get("value"))
                .and_then(|v| v.as_str())
            {
                aliases.insert(label.to_string(), true);
            }
        }

        aliases.into_keys().collect()
    }

    /// Returns the best quantity string from a list of claims for a quantity property.
    pub fn get_best_quantity(claims: &[Value]) -> Option<String> {
        let dv = claims.first()?.get("mainsnak")?.get("datavalue")?;
        let amount_str = dv.get("value")?.get("amount")?.as_str()?;
        let amount_str = amount_str.trim_start_matches('+');
        if let Ok(val) = amount_str.parse::<f64>() {
            if val >= 1_000_000.0 {
                let millions = (val / 1_000_000.0 * 10.0).round() / 10.0;
                return Some(format!("{}M", millions));
            }
            // Integers without decimal point, floats with.
            if val == val.floor() {
                return Some(format!("{}", val as i64));
            }
            return Some(format!("{}", val));
        }
        Some(amount_str.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sanitize_q() {
        assert_eq!(sanitize_q("12345"), "Q12345");
        assert_eq!(sanitize_q("Q42"), "Q42");
        assert_eq!(sanitize_q("q42"), "Q42");
        assert_eq!(sanitize_q("  Q42  "), "Q42");
    }

    #[test]
    fn test_unified_id() {
        assert_eq!(unified_id("p31"), "P31");
        assert_eq!(unified_id("Q 42"), "Q42");
    }

    #[test]
    fn test_placeholder() {
        let item = WikiDataItem::placeholder();
        assert!(item.is_placeholder());
        assert_eq!(item.get_id(), "");
        assert_eq!(item.get_label(None), "");
        assert_eq!(item.get_label(Some("en")), "");
        assert_eq!(item.get_desc(Some("en")), "");
        assert!(!item.has_claims("P31"));
        assert!(item.get_claim_items_for_property("P31").is_empty());
        assert!(item.get_strings_for_property("P18").is_empty());
        assert!(item.get_wiki_links().is_empty());
    }

    #[test]
    fn test_new_with_null_is_placeholder() {
        let item = WikiDataItem::new(serde_json::Value::Null);
        assert!(item.is_placeholder());
    }

    #[test]
    fn test_get_label_specific_language() {
        let raw = serde_json::json!({
            "id": "Q42",
            "ns": 0,
            "labels": {
                "en": { "value": "Douglas Adams" },
                "de": { "value": "Douglas Adams (de)" }
            }
        });
        let item = WikiDataItem::new(raw);
        assert_eq!(item.get_label(Some("en")), "Douglas Adams");
        assert_eq!(item.get_label(Some("de")), "Douglas Adams (de)");
        // Missing language falls back to id
        assert_eq!(item.get_label(Some("xx")), "Q42");
    }

    #[test]
    fn test_get_label_fallback_to_mul() {
        let raw = serde_json::json!({
            "id": "Q42",
            "ns": 0,
            "labels": {
                "mul": { "value": "Mul Label" }
            }
        });
        let item = WikiDataItem::new(raw);
        // Specific language not present → falls back to "mul"
        assert_eq!(item.get_label(Some("fr")), "Mul Label");
        // No language → MAIN_LANGUAGES all absent, falls back to "mul"
        assert_eq!(item.get_label(None), "Mul Label");
    }

    #[test]
    fn test_get_label_fallback_to_main_language() {
        let raw = serde_json::json!({
            "id": "Q42",
            "ns": 0,
            "labels": {
                // Only has "de" which is second in MAIN_LANGUAGES
                "de": { "value": "Deutsch Label" }
            }
        });
        let item = WikiDataItem::new(raw);
        assert_eq!(item.get_label(None), "Deutsch Label");
    }

    #[test]
    fn test_get_label_fallback_any_language() {
        let raw = serde_json::json!({
            "id": "Q999",
            "ns": 0,
            "labels": {
                "sw": { "value": "Swahili Label" }
            }
        });
        let item = WikiDataItem::new(raw);
        // Not in MAIN_LANGUAGES; should still return the only available label
        assert_eq!(item.get_label(None), "Swahili Label");
    }

    #[test]
    fn test_get_desc() {
        let raw = serde_json::json!({
            "id": "Q42",
            "descriptions": {
                "en": { "value": "English author" },
                "de": { "value": "Englischer Autor" }
            }
        });
        let item = WikiDataItem::new(raw);
        assert_eq!(item.get_desc(Some("en")), "English author");
        assert_eq!(item.get_desc(Some("de")), "Englischer Autor");
        assert_eq!(item.get_desc(Some("xx")), "");
        // With None: first main language that has a desc (en)
        assert_eq!(item.get_desc(None), "English author");
    }

    #[test]
    fn test_get_claims_for_property() {
        let raw = serde_json::json!({
            "claims": {
                "P31": [
                    { "mainsnak": { "datavalue": { "value": { "entity-type": "item", "id": "Q5" } } } }
                ]
            }
        });
        let item = WikiDataItem::new(raw);
        let claims = item.get_claims_for_property("P31");
        assert_eq!(claims.len(), 1);
        // Case-insensitive lookup via unified_id
        let claims_lower = item.get_claims_for_property("p31");
        assert_eq!(claims_lower.len(), 1);
    }

    #[test]
    fn test_has_claims() {
        let raw = serde_json::json!({
            "claims": {
                "P31": [{}]
            }
        });
        let item = WikiDataItem::new(raw);
        assert!(item.has_claims("P31"));
        assert!(!item.has_claims("P21"));
    }

    #[test]
    fn test_get_claim_target_item_id_with_id_field() {
        let claim = serde_json::json!({
            "mainsnak": {
                "datavalue": {
                    "value": {
                        "entity-type": "item",
                        "id": "Q42"
                    }
                }
            }
        });
        assert_eq!(
            WikiDataItem::get_claim_target_item_id(&claim),
            Some("Q42".to_string())
        );
    }

    #[test]
    fn test_get_claim_target_item_id_with_numeric_id() {
        let claim = serde_json::json!({
            "mainsnak": {
                "datavalue": {
                    "value": {
                        "entity-type": "item",
                        "numeric-id": 42
                    }
                }
            }
        });
        assert_eq!(
            WikiDataItem::get_claim_target_item_id(&claim),
            Some("Q42".to_string())
        );
    }

    #[test]
    fn test_get_claim_target_item_id_wrong_entity_type() {
        let claim = serde_json::json!({
            "mainsnak": {
                "datavalue": {
                    "value": {
                        "entity-type": "property",
                        "id": "P31"
                    }
                }
            }
        });
        assert_eq!(WikiDataItem::get_claim_target_item_id(&claim), None);
    }

    #[test]
    fn test_get_claim_target_string() {
        let claim = serde_json::json!({
            "mainsnak": {
                "datavalue": {
                    "type": "string",
                    "value": "hello"
                }
            }
        });
        assert_eq!(
            WikiDataItem::get_claim_target_string(&claim),
            Some("hello".to_string())
        );
    }

    #[test]
    fn test_get_claim_target_string_wrong_type() {
        let claim = serde_json::json!({
            "mainsnak": {
                "datavalue": {
                    "type": "wikibase-entityid",
                    "value": { "entity-type": "item", "id": "Q42" }
                }
            }
        });
        assert_eq!(WikiDataItem::get_claim_target_string(&claim), None);
    }

    #[test]
    fn test_get_claim_date() {
        let claim = serde_json::json!({
            "mainsnak": {
                "datavalue": {
                    "type": "time",
                    "value": { "time": "+1952-03-11T00:00:00Z", "precision": 11 }
                }
            }
        });
        let date = WikiDataItem::get_claim_date(&claim);
        assert!(date.is_some());
        let d = date.unwrap();
        assert_eq!(
            d.get("time").and_then(|t| t.as_str()),
            Some("+1952-03-11T00:00:00Z")
        );
    }

    #[test]
    fn test_has_claim_item_link() {
        let raw = serde_json::json!({
            "claims": {
                "P31": [{
                    "mainsnak": {
                        "datavalue": {
                            "value": {
                                "entity-type": "item",
                                "id": "Q5"
                            }
                        }
                    }
                }]
            }
        });
        let item = WikiDataItem::new(raw);
        assert!(item.has_claim_item_link("P31", "Q5"));
        assert!(!item.has_claim_item_link("P31", "Q42"));
        assert!(!item.has_claim_item_link("P21", "Q5"));
    }

    #[test]
    fn test_get_claim_items_for_property() {
        let raw = serde_json::json!({
            "claims": {
                "P31": [
                    {
                        "mainsnak": {
                            "datavalue": {
                                "value": { "entity-type": "item", "id": "Q5" }
                            }
                        }
                    },
                    {
                        "mainsnak": {
                            "datavalue": {
                                "value": { "entity-type": "item", "id": "Q6256" }
                            }
                        }
                    }
                ]
            }
        });
        let item = WikiDataItem::new(raw);
        let ids = item.get_claim_items_for_property("P31");
        assert_eq!(ids.len(), 2);
        assert!(ids.contains(&"Q5".to_string()));
        assert!(ids.contains(&"Q6256".to_string()));
    }

    #[test]
    fn test_get_gendered_label() {
        let raw = serde_json::json!({
            "claims": {
                "P2521": [
                    {
                        "mainsnak": {
                            "datavalue": {
                                "type": "monolingualtext",
                                "value": { "text": "escritora", "language": "es" }
                            }
                        }
                    }
                ],
                "P3321": [
                    {
                        "mainsnak": {
                            "datavalue": {
                                "type": "monolingualtext",
                                "value": { "text": "escritor", "language": "es" }
                            }
                        }
                    }
                ]
            }
        });
        let item = WikiDataItem::new(raw);
        assert_eq!(item.get_gendered_label("es", true), Some("escritora".to_string()));
        assert_eq!(item.get_gendered_label("es", false), Some("escritor".to_string()));
        assert_eq!(item.get_gendered_label("fr", true), None);
    }

    #[test]
    fn test_get_demonym() {
        let raw = serde_json::json!({
            "claims": {
                "P1549": [
                    {
                        "mainsnak": {
                            "datavalue": {
                                "type": "monolingualtext",
                                "value": { "text": "British", "language": "en" }
                            }
                        }
                    },
                    {
                        "mainsnak": {
                            "datavalue": {
                                "type": "monolingualtext",
                                "value": { "text": "Britannique", "language": "fr" }
                            }
                        }
                    }
                ]
            }
        });
        let item = WikiDataItem::new(raw);
        assert_eq!(item.get_demonym("en"), Some("British".to_string()));
        assert_eq!(item.get_demonym("fr"), Some("Britannique".to_string()));
        assert_eq!(item.get_demonym("de"), None);
    }

    #[test]
    fn test_get_strings_for_property() {
        let raw = serde_json::json!({
            "claims": {
                "P345": [
                    {
                        "mainsnak": {
                            "datavalue": {
                                "type": "string",
                                "value": "tt0080684"
                            }
                        }
                    }
                ]
            }
        });
        let item = WikiDataItem::new(raw);
        let strings = item.get_strings_for_property("P345");
        assert_eq!(strings, vec!["tt0080684"]);
    }

    #[test]
    fn test_get_wiki_links() {
        let raw = serde_json::json!({
            "sitelinks": {
                "enwiki": { "title": "Douglas Adams" },
                "dewiki": { "title": "Douglas Adams" }
            }
        });
        let item = WikiDataItem::new(raw);
        let links = item.get_wiki_links();
        assert!(links.contains_key("enwiki"));
        assert!(links.contains_key("dewiki"));
        assert!(!links.contains_key("frwiki"));
    }

    #[test]
    fn test_get_aliases_for_language() {
        let raw = serde_json::json!({
            "labels": {
                "en": { "value": "DNA" }
            },
            "aliases": {
                "en": [
                    { "value": "Douglas Noel Adams" },
                    { "value": "D.N. Adams" }
                ]
            }
        });
        let item = WikiDataItem::new(raw);

        let aliases_no_label = item.get_aliases_for_language("en", false);
        assert_eq!(aliases_no_label.len(), 2);
        assert!(aliases_no_label.contains(&"Douglas Noel Adams".to_string()));

        let aliases_with_label = item.get_aliases_for_language("en", true);
        assert_eq!(aliases_with_label.len(), 3);
        assert!(aliases_with_label.contains(&"DNA".to_string()));
    }

    #[test]
    fn test_get_best_quantity_millions() {
        let claims = serde_json::json!([{
            "mainsnak": {
                "datavalue": {
                    "value": { "amount": "+1500000" }
                }
            }
        }]);
        let arr = claims.as_array().unwrap();
        assert_eq!(
            WikiDataItem::get_best_quantity(arr),
            Some("1.5M".to_string())
        );
    }

    #[test]
    fn test_get_best_quantity_integer() {
        let claims = serde_json::json!([{
            "mainsnak": {
                "datavalue": {
                    "value": { "amount": "+42" }
                }
            }
        }]);
        let arr = claims.as_array().unwrap();
        assert_eq!(WikiDataItem::get_best_quantity(arr), Some("42".to_string()));
    }

    #[test]
    fn test_get_best_quantity_float() {
        let claims = serde_json::json!([{
            "mainsnak": {
                "datavalue": {
                    "value": { "amount": "+3.14" }
                }
            }
        }]);
        let arr = claims.as_array().unwrap();
        assert_eq!(
            WikiDataItem::get_best_quantity(arr),
            Some("3.14".to_string())
        );
    }

    #[test]
    fn test_get_best_quantity_empty() {
        assert_eq!(WikiDataItem::get_best_quantity(&[]), None);
    }

    #[test]
    fn test_get_best_quantity_exactly_one_million() {
        let claims = serde_json::json!([{
            "mainsnak": {
                "datavalue": {
                    "value": { "amount": "+1000000" }
                }
            }
        }]);
        let arr = claims.as_array().unwrap();
        assert_eq!(WikiDataItem::get_best_quantity(arr), Some("1M".to_string()));
    }
}
