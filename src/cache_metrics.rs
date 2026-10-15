//! Cache metrics for the service dashboard. No-ops unless telemetry is running.

/// The shared Wikidata item cache.
pub const ITEM: &str = "item";
/// The rendered-response cache.
pub const OUTPUT: &str = "output";

/// Counts one lookup as `autodesc_cache_requests_total{cache,result}`.
pub fn record_lookup(cache: &'static str, hit: bool) {
    let result = if hit { "hit" } else { "miss" };
    wikimisc::metrics::counter!("autodesc_cache_requests_total", "cache" => cache, "result" => result)
        .increment(1);
}

/// Sets `autodesc_cache_entries{cache}`.
pub fn record_entries(cache: &'static str, entries: u64) {
    wikimisc::metrics::gauge!("autodesc_cache_entries", "cache" => cache).set(entries as f64);
}

/// Sets `autodesc_cache_bytes{cache}`, the estimated memory held by a weighted cache.
pub fn record_bytes(cache: &'static str, bytes: u64) {
    wikimisc::metrics::gauge!("autodesc_cache_bytes", "cache" => cache).set(bytes as f64);
}
