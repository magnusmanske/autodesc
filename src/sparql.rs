//! The Wikidata Query Service.
//!
//! WDQS v1 (Blazegraph) for now: v2 (`SparqlEndpoint::wikidata_main`) refuses
//! user agents it has not allow-listed until its public launch. The queries are
//! plain SPARQL 1.1, with no label service, so they run on both.

use std::sync::OnceLock;
use wikimisc::sparql_client::SparqlClient;
use wikimisc::sparql_endpoint::SparqlEndpoint;

/// What SPARQL requests say they are. Wikimedia asks for contact information.
const USER_AGENT: &str = concat!(
    env!("CARGO_PKG_NAME"),
    "/",
    env!("CARGO_PKG_VERSION"),
    " (",
    env!("CARGO_PKG_REPOSITORY"),
    ")"
);

/// Retries of a query that failed in a way that may go away on its own.
/// One only: a request is waiting.
const RETRIES: u32 = 1;

/// The shared SPARQL client.
pub fn client() -> anyhow::Result<&'static SparqlClient> {
    static CLIENT: OnceLock<SparqlClient> = OnceLock::new();
    if let Some(client) = CLIENT.get() {
        return Ok(client);
    }
    let client = SparqlClient::new(SparqlEndpoint::wdqs(), USER_AGENT)?.with_retries(RETRIES);
    Ok(CLIENT.get_or_init(|| client))
}
