use std::collections::{HashMap, HashSet};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, OnceLock};
use std::time::Duration;

use moka::future::Cache;
use regex::Regex;
use reqwest::Client;
use serde_json::Value;
use tokio::sync::Semaphore;

use crate::qid::QId;
pub use crate::wikidata_item::{MAIN_LANGUAGES, WikiDataItem};

fn global_client() -> &'static Client {
    static CLIENT: OnceLock<Client> = OnceLock::new();
    CLIENT.get_or_init(|| {
        let pool_max_idle = std::env::var("AUTODESC_REQWEST_POOL_IDLE")
            .ok()
            .and_then(|v| v.parse::<usize>().ok())
            .unwrap_or(128);

        Client::builder()
            .user_agent("autodesc/0.2.0 (https://github.com/magnusmanske/autodesc; magnusmanske@googlemail.com) reqwest")
            .connect_timeout(Duration::from_secs(5))
            .timeout(Duration::from_secs(30))
            .pool_max_idle_per_host(pool_max_idle)
            .pool_idle_timeout(Duration::from_secs(90))
            .tcp_keepalive(Duration::from_secs(60))
            .build()
            .expect("Failed to build HTTP client")
    })
}

fn year_regex() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"^([+-])0*(\d+)").expect("year regex is valid"))
}

static SEMAPHORE_LIMIT: AtomicUsize = AtomicUsize::new(500);

/// Set the maximum number of concurrent Wikidata API requests.
/// Must be called before the semaphore is first used to have any effect.
pub fn set_semaphore_limit(n: usize) {
    SEMAPHORE_LIMIT.store(n, Ordering::Relaxed);
}

fn get_semaphore() -> &'static Arc<Semaphore> {
    static SEM: OnceLock<Arc<Semaphore>> = OnceLock::new();
    SEM.get_or_init(|| Arc::new(Semaphore::new(SEMAPHORE_LIMIT.load(Ordering::Relaxed))))
}

/// How long to wait for a Wikidata API semaphore permit before giving up.
fn semaphore_timeout() -> Duration {
    static TIMEOUT: OnceLock<Duration> = OnceLock::new();
    *TIMEOUT.get_or_init(|| {
        let secs = std::env::var("AUTODESC_SEMAPHORE_TIMEOUT_SECS")
            .ok()
            .and_then(|v| v.parse::<u64>().ok())
            .unwrap_or(10);
        Duration::from_secs(secs)
    })
}

/// Retry configuration for Wikidata API calls.
const MAX_RETRIES: u32 = 3;
const BASE_RETRY_DELAY_MS: u64 = 250;

/// Error returned when the semaphore cannot be acquired in time.
#[derive(Debug, thiserror::Error)]
pub enum ApiError {
    #[error("Too many concurrent Wikidata API requests; try again later")]
    SemaphoreTimeout,
    #[error("Wikidata API error: {0}")]
    Api(#[from] anyhow::Error),
}

/// Execute a fallible async operation with exponential backoff retry.
async fn with_retry<F, Fut, T>(operation: F) -> Result<T, anyhow::Error>
where
    F: Fn() -> Fut,
    Fut: std::future::Future<Output = Result<T, anyhow::Error>>,
{
    let mut last_error = None;
    for attempt in 0..MAX_RETRIES {
        match operation().await {
            Ok(result) => return Ok(result),
            Err(e) => {
                tracing::warn!(attempt, error = %e, "Wikidata API call failed, retrying");
                last_error = Some(e);
                if attempt + 1 < MAX_RETRIES {
                    let delay_ms = BASE_RETRY_DELAY_MS * 2u64.pow(attempt);
                    tokio::time::sleep(Duration::from_millis(delay_ms)).await;
                }
            }
        }
    }
    Err(last_error.unwrap_or_else(|| anyhow::anyhow!("Retry exhausted with no error")))
}

/// The main Wikidata client that fetches and caches entities.
pub struct WikiData {
    pub items: HashMap<QId, WikiDataItem>,
    client: Client,
    api_url: String,
    max_get_entities: usize,
    /// Optional shared global item cache.
    item_cache: Option<Cache<QId, WikiDataItem>>,
}

impl WikiData {
    pub fn new() -> Self {
        Self::with_api_url("https://www.wikidata.org/w/api.php")
    }

    pub fn with_api_url(api_url: &str) -> Self {
        Self {
            items: HashMap::new(),
            client: global_client().clone(),
            api_url: api_url.to_string(),
            max_get_entities: 50,
            item_cache: None,
        }
    }

    /// Attach a shared global item cache. Items will be read from and written to it.
    pub fn with_item_cache(mut self, cache: Cache<QId, WikiDataItem>) -> Self {
        self.item_cache = Some(cache);
        self
    }

    pub fn has_item(&self, q: &QId) -> bool {
        self.items.contains_key(q)
    }

    pub fn get_item(&self, q: &QId) -> Option<&WikiDataItem> {
        self.items.get(q)
    }

    pub fn clear(&mut self) {
        self.items.clear();
    }

    /// Fetch a batch of entities from the Wikidata API.
    /// Entities already in the local map or global item cache are skipped.
    pub async fn get_item_batch(&mut self, item_list: &[String]) -> anyhow::Result<()> {
        let mut to_load: Vec<QId> = Vec::new();
        let mut seen = HashSet::new();

        for q_raw in item_list {
            let q = match QId::parse(q_raw) {
                Ok(q) => q,
                Err(_) => continue,
            };
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

    async fn load_item_chunk(&mut self, chunk: &[QId]) -> Result<(), anyhow::Error> {
        let ids: String = chunk
            .iter()
            .map(|q| q.as_str())
            .collect::<Vec<_>>()
            .join("|");
        let params = [
            ("action", "wbgetentities"),
            ("ids", &ids),
            (
                "props",
                "info|aliases|labels|descriptions|claims|sitelinks|datatype",
            ),
            ("format", "json"),
        ];

        // Acquire semaphore with timeout so we don't block indefinitely.
        let _permit =
            match tokio::time::timeout(semaphore_timeout(), get_semaphore().acquire()).await {
                Ok(Ok(permit)) => permit,
                Ok(Err(_)) => {
                    return Err(anyhow::anyhow!("Semaphore closed"));
                }
                Err(_elapsed) => {
                    tracing::warn!(
                        "Semaphore acquisition timed out for chunk of {} items",
                        chunk.len()
                    );
                    return Err(anyhow::anyhow!(
                        "Too many concurrent Wikidata API requests; try again later"
                    ));
                }
            };

        // Use retry logic for the API call itself.
        let api_url = self.api_url.clone();
        let client = self.client.clone();

        let resp = with_retry(|| {
            let api_url = api_url.clone();
            let client = client.clone();

            async move {
                let resp = client
                    .get(&api_url)
                    .query(&params)
                    .send()
                    .await?
                    .json::<Value>()
                    .await?;
                Ok(resp)
            }
        })
        .await?;

        if let Some(entities) = resp.get("entities").and_then(|e| e.as_object()) {
            for (k, v) in entities {
                let q = QId::from_api_key(k);
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
        let q = QId::parse(q).map_err(|e| anyhow::anyhow!("Invalid Q-id: {e}"))?;
        self.get_item_batch(&[q.as_str().to_string()]).await
    }

    /// Fetch JSON from an arbitrary URL via GET with query params.
    pub async fn get_json_params(
        &self,
        url: &str,
        params: &[(&str, &str)],
    ) -> anyhow::Result<Value> {
        let client = self.client.clone();
        let url = url.to_string();
        let params: Vec<(String, String)> = params
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect();

        with_retry(|| {
            let client = client.clone();
            let url = url.clone();
            let params = params.clone();

            async move {
                let resp = client
                    .get(&url)
                    .query(&params)
                    .send()
                    .await?
                    .json::<Value>()
                    .await?;
                Ok(resp)
            }
        })
        .await
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
        assert!(wd.has_item(&QId::parse("Q12345").unwrap()));

        let item = wd.get_item(&QId::parse("Q12345").unwrap()).unwrap();
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
        assert!(wd.has_item(&QId::parse("Q42").unwrap()));
        assert!(wd.has_item(&QId::parse("Q1").unwrap()));
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
        assert!(wd.has_item(&QId::parse("Q42").unwrap()));
        wd.clear();
        assert!(!wd.has_item(&QId::parse("Q42").unwrap()));
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
        assert!(wd.has_item(&QId::parse("Q42").unwrap()));
        assert!(wd.has_item(&QId::parse("Q1").unwrap()));
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
        assert!(wd.has_item(&QId::parse("Q42").unwrap()));
    }
}
