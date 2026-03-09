use regex::Regex;
use reqwest::Client;
use serde_json::Value;
use std::collections::HashMap;

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
        let main_languages = [
            "en", "de", "fr", "nl", "es", "it", "pl", "pt", "ja", "ru", "hu", "sv", "fi",
        ];

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
            return fallback;
        }

        // No language specified: try main languages, then any
        for lang in &main_languages {
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

        // Fallback: pick first available language
        if let Some(labels) = self.raw.get("labels").and_then(|l| l.as_object()) {
            for (_lang, val) in labels {
                if let Some(label) = val.get("value").and_then(|v| v.as_str()) {
                    return label.to_string();
                }
            }
        }

        fallback
    }

    pub fn get_desc(&self, language: Option<&str>) -> String {
        let main_languages = [
            "en", "de", "fr", "nl", "es", "it", "pl", "pt", "ja", "ru", "hu", "sv", "fi",
        ];

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

        for lang in &main_languages {
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

    pub fn get_claim_target_item_id(claim: &Value) -> Option<String> {
        let nid = claim.get("mainsnak")?.get("datavalue")?.get("value")?;

        // Check entity-type == "item"
        let entity_type = nid.get("entity-type").and_then(|v| v.as_str())?;
        if entity_type != "item" {
            return None;
        }

        // Try "id" field first (newer format), then "numeric-id"
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
        let claims = self.get_claims_for_property(p);
        for claim in &claims {
            if let Some(id) = Self::get_claim_target_item_id(claim) {
                if id == q {
                    return true;
                }
            }
        }
        false
    }

    pub fn get_claim_items_for_property(&self, p: &str) -> Vec<String> {
        let claims = self.get_claims_for_property(p);
        let mut ret = Vec::new();
        for claim in &claims {
            if let Some(q) = Self::get_claim_target_item_id(claim) {
                ret.push(q);
            }
        }
        ret
    }

    pub fn get_strings_for_property(&self, p: &str) -> Vec<String> {
        let claims = self.get_claims_for_property(p);
        let mut ret = Vec::new();
        for claim in &claims {
            if let Some(s) = Self::get_claim_target_string(claim) {
                ret.push(s);
            }
        }
        ret
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
            // Format nicely: integers without decimal, floats with decimals
            if val == val.floor() {
                return Some(format!("{}", val as i64));
            }
            return Some(format!("{}", val));
        }
        Some(amount_str.to_string())
    }
}

/// Normalize an entity ID to uppercase with proper prefix.
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

/// The main Wikidata client that fetches and caches entities.
pub struct WikiData {
    pub items: HashMap<String, WikiDataItem>,
    client: Client,
    api_url: String,
    max_get_entities: usize,
}

impl WikiData {
    pub fn new() -> Self {
        let client = Client::builder()
            .user_agent("autodesc/0.2.0 (https://github.com/magnusmanske/autodesc; magnusmanske@googlemail.com) reqwest")
            .build()
            .expect("Failed to build HTTP client");
        Self {
            items: HashMap::new(),
            client,
            api_url: "https://www.wikidata.org/w/api.php".to_string(),
            max_get_entities: 50,
        }
    }

    pub fn has_item(&self, q: &str) -> bool {
        self.items.contains_key(&unified_id(q))
    }

    pub fn get_item(&self, q: &str) -> Option<&WikiDataItem> {
        self.items.get(&unified_id(q))
    }

    pub fn clear(&mut self) {
        self.items.clear();
    }

    /// Fetch a batch of entities from the Wikidata API.
    /// Entities already in the cache are skipped.
    pub async fn get_item_batch(&mut self, item_list: &[String]) -> anyhow::Result<()> {
        let mut to_load: Vec<String> = Vec::new();
        let mut seen = std::collections::HashSet::new();

        for q in item_list {
            let q = sanitize_q(q);
            if self.items.contains_key(&q) || seen.contains(&q) {
                continue;
            }
            seen.insert(q.clone());
            to_load.push(q);
        }

        if to_load.is_empty() {
            return Ok(());
        }

        // Split into batches
        for chunk in to_load.chunks(self.max_get_entities) {
            let ids = chunk.join("|");
            let params = [
                ("action", "wbgetentities"),
                ("ids", &ids),
                (
                    "props",
                    "info|aliases|labels|descriptions|claims|sitelinks|datatype",
                ),
                ("format", "json"),
            ];

            let resp = self
                .client
                .post(&self.api_url)
                .form(&params)
                .send()
                .await?
                .json::<Value>()
                .await?;

            if let Some(entities) = resp.get("entities").and_then(|e| e.as_object()) {
                for (k, v) in entities {
                    let q = unified_id(k);
                    self.items.insert(q, WikiDataItem::new(v.clone()));
                }
            }
        }

        Ok(())
    }

    /// Convenience: load a single entity by Q-id.
    pub async fn load_entity(&mut self, q: &str) -> anyhow::Result<()> {
        let q = sanitize_q(q);
        self.get_item_batch(&[q]).await
    }

    /// Fetch JSON from an arbitrary URL via POST.
    pub async fn post_json(&self, url: &str, params: &[(&str, &str)]) -> anyhow::Result<Value> {
        let resp = self
            .client
            .post(url)
            .form(params)
            .send()
            .await?
            .json::<Value>()
            .await?;
        Ok(resp)
    }

    /// Fetch JSON from an arbitrary URL via GET.
    pub async fn get_json(&self, url: &str) -> anyhow::Result<Value> {
        let resp = self.client.get(url).send().await?.json::<Value>().await?;
        Ok(resp)
    }

    /// Extract a year string from a set of claims for a time-valued property.
    /// `p` is a numeric property id (e.g. 569 for P569).
    pub fn get_year(
        claims: &Value,
        p: u64,
        lang: &str,
        stock: &HashMap<String, HashMap<String, String>>,
    ) -> String {
        let prop = format!("P{}", p);
        let claims_arr = match claims.get(&prop).and_then(|v| v.as_array()) {
            Some(arr) => arr,
            None => return String::new(),
        };

        let re = Regex::new(r"^([+-])0*(\d+)").unwrap();

        for claim in claims_arr {
            let time_str = claim
                .get("mainsnak")
                .and_then(|ms| ms.get("datavalue"))
                .and_then(|dv| dv.get("value"))
                .and_then(|v| v.get("time"))
                .and_then(|t| t.as_str());

            if let Some(time_str) = time_str {
                if let Some(caps) = re.captures(time_str) {
                    let sign = caps.get(1).map(|m| m.as_str()).unwrap_or("+");
                    let year = caps.get(2).map(|m| m.as_str()).unwrap_or("");
                    let mut ret = year.to_string();
                    if sign == "-" {
                        let bc = stock
                            .get("BC")
                            .and_then(|m| m.get(lang).or_else(|| m.get("en")))
                            .map(|s| s.as_str())
                            .unwrap_or("BC");
                        ret.push_str(bc);
                    }
                    return ret;
                }
            }
        }

        String::new()
    }
}

impl Default for WikiData {
    fn default() -> Self {
        Self::new()
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

    #[tokio::test]
    async fn test_load_entity() {
        let mut wd = WikiData::new();
        wd.load_entity("Q12345").await.unwrap();
        assert!(wd.has_item("Q12345"));

        let item = wd.get_item("Q12345").unwrap();
        assert!(!item.is_placeholder());

        let label = item.get_label(Some("en"));
        assert!(!label.is_empty());

        // Test IMDB strings
        let imdb = item.get_strings_for_property("P345");
        assert!(!imdb.is_empty());

        // Test has_claim_item_link
        assert!(item.has_claim_item_link("P31", "Q30061417"));

        // Test sitelinks
        let wl = item.get_wiki_links();
        assert!(wl.contains_key("dewiki"));
    }

    #[tokio::test]
    async fn test_batch_loading() {
        let mut wd = WikiData::new();
        let items = vec!["Q42".to_string(), "Q1".to_string()];
        wd.get_item_batch(&items).await.unwrap();
        assert!(wd.has_item("Q42"));
        assert!(wd.has_item("Q1"));
    }

    #[test]
    fn test_placeholder() {
        let item = WikiDataItem::placeholder();
        assert!(item.is_placeholder());
        assert_eq!(item.get_id(), "");
        assert_eq!(item.get_label(None), "");
        assert!(!item.has_claims("P31"));
    }
}
