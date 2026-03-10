use std::collections::{HashMap, HashSet};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, OnceLock};

use moka::future::Cache;
use regex::Regex;
use reqwest::Client;
use serde_json::Value;
use tokio::sync::Semaphore;

pub use crate::wikidata_item::{MAIN_LANGUAGES, WikiDataItem, sanitize_q, unified_id};

fn year_regex() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"^([+-])0*(\d+)").expect("year regex is valid"))
}

static SEMAPHORE_LIMIT: AtomicUsize = AtomicUsize::new(usize::MAX);

/// Set the maximum number of concurrent Wikidata API requests.
/// Must be called before the semaphore is first used to have any effect.
pub fn set_semaphore_limit(n: usize) {
    SEMAPHORE_LIMIT.store(n, Ordering::Relaxed);
}

fn get_semaphore() -> &'static Arc<Semaphore> {
    static SEM: OnceLock<Arc<Semaphore>> = OnceLock::new();
    SEM.get_or_init(|| Arc::new(Semaphore::new(SEMAPHORE_LIMIT.load(Ordering::Relaxed))))
}

/// The main Wikidata client that fetches and caches entities.
pub struct WikiData {
    pub items: HashMap<String, WikiDataItem>,
    client: Client,
    api_url: String,
    max_get_entities: usize,
    /// Optional shared global item cache.
    item_cache: Option<Cache<String, WikiDataItem>>,
}

impl WikiData {
    pub fn new() -> Self {
        Self::with_api_url("https://www.wikidata.org/w/api.php")
    }

    pub fn with_api_url(api_url: &str) -> Self {
        let client = Client::builder()
            .user_agent("autodesc/0.2.0 (https://github.com/magnusmanske/autodesc; magnusmanske@googlemail.com) reqwest")
            .build()
            .expect("Failed to build HTTP client");
        Self {
            items: HashMap::new(),
            client,
            api_url: api_url.to_string(),
            max_get_entities: 50,
            item_cache: None,
        }
    }

    /// Attach a shared global item cache. Items will be read from and written to it.
    pub fn with_item_cache(mut self, cache: Cache<String, WikiDataItem>) -> Self {
        self.item_cache = Some(cache);
        self
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
    /// Entities already in the local map or global item cache are skipped.
    pub async fn get_item_batch(&mut self, item_list: &[String]) -> anyhow::Result<()> {
        let mut to_load: Vec<String> = Vec::new();
        let mut seen = HashSet::new();

        for q in item_list {
            let q = sanitize_q(q);
            if self.items.contains_key(&q) || seen.contains(&q) {
                continue;
            }
            // Check global item cache before scheduling an API fetch.
            if let Some(cache) = &self.item_cache
                && let Some(item) = cache.get(&q).await
            {
                self.items.insert(q.clone(), item);
                seen.insert(q);
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
            self.load_item_chunk(chunk).await?;
        }

        Ok(())
    }

    async fn load_item_chunk(&mut self, chunk: &[String]) -> Result<(), anyhow::Error> {
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
        let _permit = get_semaphore().acquire().await?;
        let resp = self
            .client
            .get(&self.api_url)
            .query(&params)
            .send()
            .await?
            .json::<Value>()
            .await?;
        drop(_permit);

        if let Some(entities) = resp.get("entities").and_then(|e| e.as_object()) {
            for (k, v) in entities {
                let q = unified_id(k);
                let item = WikiDataItem::new(v.clone());
                // Populate global item cache with freshly loaded items.
                if let Some(cache) = &self.item_cache {
                    cache.insert(q.clone(), item.clone()).await;
                }
                self.items.insert(q, item);
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
    pub async fn get_json_params(
        &self,
        url: &str,
        params: &[(&str, &str)],
    ) -> anyhow::Result<Value> {
        let resp = self
            .client
            .get(url)
            .query(params)
            .send()
            .await?
            .json::<Value>()
            .await?;
        Ok(resp)
    }

    /// Fetch JSON from an arbitrary URL via GET.
    pub async fn get_json(&self, url: &str) -> anyhow::Result<Value> {
        self.get_json_params(url, &[]).await
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

            if let Some(time_str) = time_str
                && let Some(caps) = re.captures(time_str)
            {
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
    use wiremock::matchers::{method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

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

    /// Helper: build a wbgetentities-style response for the given entities.
    fn fake_wbgetentities(entities: Value) -> Value {
        serde_json::json!({ "entities": entities })
    }

    fn fake_q12345() -> Value {
        serde_json::json!({
            "Q12345": {
                "type": "item",
                "id": "Q12345",
                "ns": 0,
                "labels": {
                    "en": { "language": "en", "value": "Count von Count" },
                    "de": { "language": "de", "value": "Graf Zahl" }
                },
                "descriptions": {
                    "en": { "language": "en", "value": "Sesame Street character" }
                },
                "claims": {
                    "P31": [{
                        "mainsnak": {
                            "datavalue": {
                                "value": { "entity-type": "item", "id": "Q30061417" }
                            }
                        }
                    }],
                    "P345": [{
                        "mainsnak": {
                            "datavalue": {
                                "type": "string",
                                "value": "ch0000000"
                            }
                        }
                    }]
                },
                "sitelinks": {
                    "dewiki": { "site": "dewiki", "title": "Graf Zahl" },
                    "enwiki": { "site": "enwiki", "title": "Count von Count" }
                }
            }
        })
    }

    fn fake_q42_q1() -> Value {
        serde_json::json!({
            "Q42": {
                "type": "item",
                "id": "Q42",
                "ns": 0,
                "labels": { "en": { "language": "en", "value": "Douglas Adams" } },
                "descriptions": {},
                "claims": {},
                "sitelinks": {}
            },
            "Q1": {
                "type": "item",
                "id": "Q1",
                "ns": 0,
                "labels": { "en": { "language": "en", "value": "Universe" } },
                "descriptions": {},
                "claims": {},
                "sitelinks": {}
            }
        })
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn test_load_entity() {
        let mock_server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/w/api.php"))
            .respond_with(
                ResponseTemplate::new(200).set_body_json(fake_wbgetentities(fake_q12345())),
            )
            .mount(&mock_server)
            .await;

        let mut wd = WikiData::with_api_url(&format!("{}/w/api.php", mock_server.uri()));
        wd.load_entity("Q12345").await.unwrap();
        assert!(wd.has_item("Q12345"));

        let item = wd.get_item("Q12345").unwrap();
        assert!(!item.is_placeholder());

        let label = item.get_label(Some("en"));
        assert_eq!(label, "Count von Count");

        let imdb = item.get_strings_for_property("P345");
        assert_eq!(imdb, vec!["ch0000000"]);

        assert!(item.has_claim_item_link("P31", "Q30061417"));

        let wl = item.get_wiki_links();
        assert!(wl.contains_key("dewiki"));
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn test_batch_loading() {
        let mock_server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/w/api.php"))
            .respond_with(
                ResponseTemplate::new(200).set_body_json(fake_wbgetentities(fake_q42_q1())),
            )
            .mount(&mock_server)
            .await;

        let mut wd = WikiData::with_api_url(&format!("{}/w/api.php", mock_server.uri()));
        let items = vec!["Q42".to_string(), "Q1".to_string()];
        wd.get_item_batch(&items).await.unwrap();
        assert!(wd.has_item("Q42"));
        assert!(wd.has_item("Q1"));
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn test_clear() {
        let mock_server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/w/api.php"))
            .respond_with(ResponseTemplate::new(200).set_body_json(fake_wbgetentities(
                serde_json::json!({
                    "Q42": {
                        "type": "item", "id": "Q42", "ns": 0,
                        "labels": {}, "descriptions": {}, "claims": {}, "sitelinks": {}
                    }
                }),
            )))
            .mount(&mock_server)
            .await;

        let mut wd = WikiData::with_api_url(&format!("{}/w/api.php", mock_server.uri()));
        wd.load_entity("Q42").await.unwrap();
        assert!(wd.has_item("Q42"));
        wd.clear();
        assert!(!wd.has_item("Q42"));
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn test_batch_dedup() {
        let mock_server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/w/api.php"))
            .respond_with(
                ResponseTemplate::new(200).set_body_json(fake_wbgetentities(fake_q42_q1())),
            )
            .expect(1) // only one API call despite 4 input IDs
            .mount(&mock_server)
            .await;

        let mut wd = WikiData::with_api_url(&format!("{}/w/api.php", mock_server.uri()));
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

    #[tokio::test(flavor = "multi_thread")]
    async fn test_already_loaded_skipped() {
        let mock_server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/w/api.php"))
            .respond_with(ResponseTemplate::new(200).set_body_json(fake_wbgetentities(
                serde_json::json!({
                    "Q42": {
                        "type": "item", "id": "Q42", "ns": 0,
                        "labels": { "en": { "language": "en", "value": "Douglas Adams" } },
                        "descriptions": {}, "claims": {}, "sitelinks": {}
                    }
                }),
            )))
            .expect(1) // only one API call despite two load_entity calls
            .mount(&mock_server)
            .await;

        let mut wd = WikiData::with_api_url(&format!("{}/w/api.php", mock_server.uri()));
        wd.load_entity("Q42").await.unwrap();
        wd.load_entity("Q42").await.unwrap();
        assert!(wd.has_item("Q42"));
    }
}
