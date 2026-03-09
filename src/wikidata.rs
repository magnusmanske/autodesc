use std::collections::{HashMap, HashSet};
use std::sync::OnceLock;

use regex::Regex;
use reqwest::Client;
use serde_json::Value;

pub use crate::wikidata_item::{sanitize_q, unified_id, WikiDataItem, MAIN_LANGUAGES};

fn year_regex() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"^([+-])0*(\d+)").expect("year regex is valid"))
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
        let mut seen = HashSet::new();

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

        // Split into batches of at most `max_get_entities`.
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

        let re = year_regex();

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
    fn test_get_year_bc() {
        let mut stock: HashMap<String, HashMap<String, String>> = HashMap::new();
        let mut bc_map = HashMap::new();
        bc_map.insert("en".to_string(), " BC".to_string());
        stock.insert("BC".to_string(), bc_map);

        let claims = serde_json::json!({
            "P569": [{
                "mainsnak": {
                    "datavalue": {
                        "value": { "time": "-00000384-00-00T00:00:00Z" }
                    }
                }
            }]
        });
        let year = WikiData::get_year(&claims, 569, "en", &stock);
        assert_eq!(year, "384 BC");
    }

    #[test]
    fn test_get_year_ad() {
        let stock = HashMap::new();
        let claims = serde_json::json!({
            "P569": [{
                "mainsnak": {
                    "datavalue": {
                        "value": { "time": "+1952-03-11T00:00:00Z" }
                    }
                }
            }]
        });
        let year = WikiData::get_year(&claims, 569, "en", &stock);
        assert_eq!(year, "1952");
    }

    #[test]
    fn test_get_year_missing_prop() {
        let stock = HashMap::new();
        let claims = serde_json::json!({});
        let year = WikiData::get_year(&claims, 569, "en", &stock);
        assert_eq!(year, "");
    }

    #[test]
    fn test_get_year_bc_fallback_to_en() {
        // When the requested lang has no BC translation, falls back to "en"
        let mut stock: HashMap<String, HashMap<String, String>> = HashMap::new();
        let mut bc_map = HashMap::new();
        bc_map.insert("en".to_string(), " BC".to_string());
        stock.insert("BC".to_string(), bc_map);

        let claims = serde_json::json!({
            "P569": [{
                "mainsnak": {
                    "datavalue": {
                        "value": { "time": "-00000100-00-00T00:00:00Z" }
                    }
                }
            }]
        });
        // lang "fr" not in stock, should fall back to English " BC"
        let year = WikiData::get_year(&claims, 569, "fr", &stock);
        assert_eq!(year, "100 BC");
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

    #[tokio::test]
    async fn test_clear() {
        let mut wd = WikiData::new();
        wd.load_entity("Q42").await.unwrap();
        assert!(wd.has_item("Q42"));
        wd.clear();
        assert!(!wd.has_item("Q42"));
    }

    #[tokio::test]
    async fn test_batch_dedup() {
        let mut wd = WikiData::new();
        // Duplicates and case variants should be de-duped before the API call.
        let items = vec![
            "Q42".to_string(),
            "Q1".to_string(),
            "Q42".to_string(),
            "q1".to_string(),
        ];
        wd.get_item_batch(&items).await.unwrap();
        assert!(wd.has_item("Q42"));
        assert!(wd.has_item("Q1"));
    }

    #[tokio::test]
    async fn test_already_loaded_skipped() {
        let mut wd = WikiData::new();
        wd.load_entity("Q42").await.unwrap();
        // A second load should not panic or overwrite; just a no-op.
        wd.load_entity("Q42").await.unwrap();
        assert!(wd.has_item("Q42"));
    }
}
