use std::collections::{HashMap, HashSet};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, OnceLock};
use std::time::Duration;

use moka::future::Cache;
use reqwest::Client;
use serde::Deserialize;
use serde::de::DeserializeOwned;
use serde_json::Value;
use serde_json::value::RawValue;
use tokio::sync::Semaphore;
use wikimisc::date::Date;
use wikimisc::entity_id::ItemId;
use wikimisc::http::{self, HttpError, HttpRetryPolicy};

use crate::cache_metrics;
pub use crate::wikidata_item::{MAIN_LANGUAGES, WikiDataItem};

/// What requests to the Wikidata and Commons APIs say they are.
const USER_AGENT: &str =
    wikimisc::user_agent!("https://github.com/magnusmanske/autodesc; magnusmanske@googlemail.com");

fn global_client() -> &'static Client {
    static CLIENT: OnceLock<Client> = OnceLock::new();
    CLIENT.get_or_init(|| {
        let pool_max_idle = std::env::var("AUTODESC_REQWEST_POOL_IDLE")
            .ok()
            .and_then(|v| v.parse::<usize>().ok())
            .unwrap_or(128);

        http::client_builder(USER_AGENT)
            .connect_timeout(Duration::from_secs(5))
            .timeout(Duration::from_secs(30))
            .pool_max_idle_per_host(pool_max_idle)
            .pool_idle_timeout(Duration::from_secs(90))
            .tcp_keepalive(Duration::from_secs(60))
            // As before reqwest 0.13, whose TLS stack negotiates HTTP/2:
            // parallel connections, so the pool settings above apply.
            .http1_only()
            .build()
            .expect("Failed to build HTTP client")
    })
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

/// The longest wait before a retry, including one the API asks for with
/// `Retry-After`. A request that is asked to wait longer fails right away:
/// these calls serve interactive requests.
const MAX_RETRY_WAIT: Duration = Duration::from_secs(5);

/// Retries of a Wikidata/Commons API call that failed in a way that may go
/// away on its own (HTTP 408, 429 and 5xx, and connections that failed or
/// broke while sending or reading), after 250 ms and then 500 ms, or after
/// the API's `Retry-After` up to [`MAX_RETRY_WAIT`].
fn retry_policy() -> &'static HttpRetryPolicy {
    static POLICY: OnceLock<HttpRetryPolicy> = OnceLock::new();
    POLICY.get_or_init(|| {
        HttpRetryPolicy::new(2)
            .with_initial_backoff(Duration::from_millis(250))
            .with_max_wait(MAX_RETRY_WAIT)
            .on_retry(|retry| {
                tracing::warn!(retry = retry.retry, error = %retry.error, "Wikidata API call failed, retrying");
            })
    })
}

/// Error returned when the semaphore cannot be acquired in time.
#[derive(Debug, thiserror::Error)]
pub enum ApiError {
    #[error("Too many concurrent Wikidata API requests; try again later")]
    SemaphoreTimeout,
    #[error("Wikidata API error: {0}")]
    Api(#[from] anyhow::Error),
}

/// The Wikibase time precision of a year.
const PRECISION_YEAR: u64 = 9;

/// A `wbgetentities` response. Entities stay unparsed until needed, so only one
/// full entity (which can take tens of MB as a `Value`) is in memory at a time.
#[derive(Deserialize)]
struct EntitiesResponse {
    #[serde(default)]
    entities: HashMap<String, Box<RawValue>>,
}

/// The main Wikidata client that fetches and caches entities.
pub struct WikiData {
    pub items: HashMap<ItemId, WikiDataItem>,
    client: Client,
    api_url: String,
    max_get_entities: usize,
    /// Optional shared global item cache.
    item_cache: Option<Cache<ItemId, WikiDataItem>>,
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
    pub fn with_item_cache(mut self, cache: Cache<ItemId, WikiDataItem>) -> Self {
        self.item_cache = Some(cache);
        self
    }

    pub fn has_item(&self, q: &ItemId) -> bool {
        self.items.contains_key(q)
    }

    pub fn get_item(&self, q: &ItemId) -> Option<&WikiDataItem> {
        self.items.get(q)
    }

    /// The loaded item with the Q-id `q` as found in claims (`"Q42"`), if any.
    pub fn get_item_str(&self, q: &str) -> Option<&WikiDataItem> {
        ItemId::parse_lenient(q)
            .ok()
            .and_then(|q| self.items.get(&q))
    }

    pub fn clear(&mut self) {
        self.items.clear();
    }

    /// Fetch a batch of entities from the Wikidata API.
    /// Entities already in the local map or global item cache are skipped.
    pub async fn get_item_batch(&mut self, item_list: &[String]) -> anyhow::Result<()> {
        let mut to_load: Vec<ItemId> = Vec::new();
        let mut seen = HashSet::new();

        for q_raw in item_list {
            let q = match ItemId::parse_lenient(q_raw) {
                Ok(q) => q,
                Err(_) => continue,
            };
            if self.items.contains_key(&q) || seen.contains(&q) {
                continue;
            }
            // Check global item cache before scheduling an API fetch.
            if let Some(cache) = &self.item_cache {
                let cached = cache.get(&q).await;
                cache_metrics::record_lookup(cache_metrics::ITEM, cached.is_some());
                if let Some(item) = cached {
                    self.items.insert(q, item);
                    seen.insert(q);
                    continue;
                }
            }
            seen.insert(q);
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

    async fn load_item_chunk(&mut self, chunk: &[ItemId]) -> Result<(), anyhow::Error> {
        let ids: String = chunk
            .iter()
            .map(ToString::to_string)
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

        let resp: EntitiesResponse = self.fetch_json(&self.api_url, &params).await?;

        for (k, raw) in resp.entities {
            let Ok(q) = k.parse::<ItemId>() else {
                tracing::warn!("Wikidata API returned non-item key {k}");
                continue;
            };
            let item = WikiDataItem::new(serde_json::from_str(raw.get())?);
            // Populate global item cache with freshly loaded items.
            if let Some(cache) = &self.item_cache {
                cache.insert(q, item.clone()).await;
            }
            self.items.insert(q, item);
        }
        Ok(())
    }

    /// Convenience: load a single entity by Q-id.
    pub async fn load_entity(&mut self, q: &str) -> anyhow::Result<()> {
        let q = ItemId::parse_lenient(q).map_err(|e| anyhow::anyhow!("Invalid Q-id: {e}"))?;
        self.get_item_batch(&[q.to_string()]).await
    }

    /// Fetch JSON from an arbitrary URL via GET with query params.
    pub async fn get_json_params(
        &self,
        url: &str,
        params: &[(&str, &str)],
    ) -> anyhow::Result<Value> {
        Ok(self.fetch_json(url, params).await?)
    }

    /// GETs `url` with `params` and parses the JSON answer, retrying failures
    /// that may go away on their own.
    async fn fetch_json<T: DeserializeOwned>(
        &self,
        url: &str,
        params: &[(&str, &str)],
    ) -> Result<T, HttpError> {
        retry_policy()
            .fetch_json(self.client.get(url).query(params))
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

        for claim in claims_arr {
            let year = claim
                .get("mainsnak")
                .and_then(|ms| ms.get("datavalue"))
                .and_then(|dv| dv.get("value"))
                .and_then(|v| v.get("time"))
                .and_then(|t| t.as_str())
                .and_then(|time| Date::from_wikibase(time, PRECISION_YEAR).ok())
                .map(|date| date.year());

            if let Some(year) = year {
                let mut ret = year.unsigned_abs().to_string();
                if year < 0 {
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
        assert!(wd.has_item(&ItemId::parse_lenient("Q12345").unwrap()));

        let item = wd
            .get_item(&ItemId::parse_lenient("Q12345").unwrap())
            .unwrap();
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
    async fn test_wikidata_item_methods() {
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
        let item = wd
            .get_item(&ItemId::parse_lenient("Q12345").unwrap())
            .unwrap();

        assert_eq!(item.get_id(), "Q12345");
        assert!(item.is_item());
        assert!(item.has_claims("P31"));
        assert!(!item.has_claims("P21"));
        assert_eq!(item.get_claim_items_for_property("P31"), vec!["Q30061417"]);
        assert_eq!(item.get_label(Some("de")), "Graf Zahl");
        assert_eq!(item.get_label(Some("xx")), "Q12345");
        assert_eq!(item.get_label(None), "Count von Count");
        assert_eq!(item.get_desc(Some("en")), "Sesame Street character");
        assert_eq!(item.get_desc(Some("de")), "");
        assert_eq!(item.get_desc(None), "Sesame Street character");

        let links = item.get_wiki_links();
        assert_eq!(links.len(), 2);
        assert!(links.contains_key("dewiki"));
        assert!(links.contains_key("enwiki"));
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
        assert!(wd.has_item(&ItemId::parse_lenient("Q42").unwrap()));
        assert!(wd.has_item(&ItemId::parse_lenient("Q1").unwrap()));
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
        assert!(wd.has_item(&ItemId::parse_lenient("Q42").unwrap()));
        wd.clear();
        assert!(!wd.has_item(&ItemId::parse_lenient("Q42").unwrap()));
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
        assert!(wd.has_item(&ItemId::parse_lenient("Q42").unwrap()));
        assert!(wd.has_item(&ItemId::parse_lenient("Q1").unwrap()));
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn test_server_error_is_retried() {
        let mock_server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/w/api.php"))
            .respond_with(ResponseTemplate::new(503))
            .up_to_n_times(1)
            .expect(1)
            .mount(&mock_server)
            .await;
        Mock::given(method("GET"))
            .and(path("/w/api.php"))
            .respond_with(
                ResponseTemplate::new(200).set_body_json(fake_wbgetentities(fake_q42_q1())),
            )
            .expect(1)
            .mount(&mock_server)
            .await;

        let mut wd = WikiData::with_api_url(&format!("{}/w/api.php", mock_server.uri()));
        wd.load_entity("Q42").await.unwrap();
        assert!(wd.has_item(&ItemId::new(42)));
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn test_client_error_is_not_retried() {
        let mock_server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/w/api.php"))
            .respond_with(ResponseTemplate::new(404))
            .expect(1)
            .mount(&mock_server)
            .await;

        let mut wd = WikiData::with_api_url(&format!("{}/w/api.php", mock_server.uri()));
        assert!(wd.load_entity("Q42").await.is_err());
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn test_long_retry_after_fails_at_once() {
        let mock_server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/w/api.php"))
            .respond_with(ResponseTemplate::new(429).insert_header("Retry-After", "3600"))
            .expect(1)
            .mount(&mock_server)
            .await;

        let mut wd = WikiData::with_api_url(&format!("{}/w/api.php", mock_server.uri()));
        assert!(wd.load_entity("Q42").await.is_err());
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
        assert!(wd.has_item(&ItemId::parse_lenient("Q42").unwrap()));
    }
}
