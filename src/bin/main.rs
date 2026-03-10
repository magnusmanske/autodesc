use std::collections::HashMap;
use std::time::Duration;

use axum::{
    Router,
    error_handling::HandleErrorLayer,
    extract::{Query, State},
    http::StatusCode,
    response::{Html, IntoResponse, Response},
    routing::get,
};
use moka::future::Cache;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use tower::BoxError;
use tower::ServiceBuilder;
use tower_http::compression::CompressionLayer;
use tower_http::cors::CorsLayer;
use tower_http::timeout::TimeoutLayer;

use autodesc::desc_options::DescOptions;
use autodesc::media::MediaGenerator;
use autodesc::short_desc::ShortDescription;
use autodesc::wikidata::{WikiData, WikiDataItem, sanitize_q};

/// Shared application state holding both global caches.
#[derive(Clone)]
struct AppState {
    /// Cache of Wikidata items (Q-id → WikiDataItem).
    item_cache: Cache<String, WikiDataItem>,
    /// Cache of generated output strings (cache-key → output).
    output_cache: Cache<String, String>,
}

impl AppState {
    fn new() -> Self {
        let item_ttl = std::env::var("AUTODESC_ITEM_CACHE_TTL_SECS")
            .ok()
            .and_then(|v| v.parse::<u64>().ok())
            .unwrap_or(3600); // 1 hour
        let item_size = std::env::var("AUTODESC_ITEM_CACHE_SIZE")
            .ok()
            .and_then(|v| v.parse::<u64>().ok())
            .unwrap_or(10_000);

        let output_ttl = std::env::var("AUTODESC_OUTPUT_CACHE_TTL_SECS")
            .ok()
            .and_then(|v| v.parse::<u64>().ok())
            .unwrap_or(600); // 10 minutes
        let output_size = std::env::var("AUTODESC_OUTPUT_CACHE_SIZE")
            .ok()
            .and_then(|v| v.parse::<u64>().ok())
            .unwrap_or(1_000);

        tracing::info!(
            item_ttl,
            item_size,
            output_ttl,
            output_size,
            "Cache configuration"
        );

        Self {
            item_cache: Cache::builder()
                .max_capacity(item_size)
                .time_to_live(Duration::from_secs(item_ttl))
                .build(),
            output_cache: Cache::builder()
                .max_capacity(output_size)
                .time_to_live(Duration::from_secs(output_ttl))
                .build(),
        }
    }
}

const DEFAULT_LANGUAGE: &str = "en";

const INDEX_HTML: &str = include_str!("../../data/index.html");

/// Query parameters matching the Python Flask API.
#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct ApiParams {
    #[serde(default)]
    pub q: Option<String>,
    #[serde(default = "default_lang")]
    pub lang: String,
    #[serde(default = "default_mode")]
    pub mode: String,
    #[serde(default = "default_links")]
    pub links: String,
    #[serde(default)]
    pub redlinks: String,
    #[serde(default = "default_format")]
    pub format: String,
    #[serde(default = "default_get_infobox")]
    pub get_infobox: String,
    #[serde(default)]
    pub infobox_template: String,
    #[serde(default)]
    pub media: String,
    #[serde(default)]
    pub thumb: String,
    #[serde(default = "default_user_zoom")]
    pub user_zoom: u32,
    #[serde(default)]
    pub callback: Option<String>,
}

fn default_lang() -> String {
    DEFAULT_LANGUAGE.to_string()
}
fn default_mode() -> String {
    "short".to_string()
}
fn default_links() -> String {
    "text".to_string()
}
fn default_format() -> String {
    "jsonfm".to_string()
}
fn default_get_infobox() -> String {
    "yes".to_string()
}
fn default_user_zoom() -> u32 {
    4
}

/// JSON response matching the Python API output.
#[derive(Debug, Serialize)]
struct ApiResponse {
    call: Value,
    q: String,
    label: String,
    manual_description: String,
    result: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    media: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    thumbnails: Option<Value>,
}

/// Build a response from a cached JSON string, applying the requested format.
fn cached_response(cached_json: String, args: &ApiParams) -> Response {
    match args.format.as_str() {
        "html" => {
            let v: Value = serde_json::from_str(&cached_json).unwrap_or_default();
            let label = v["label"].as_str().unwrap_or("").to_string();
            let q = v["q"].as_str().unwrap_or("").to_string();
            let result = v["result"].as_str().unwrap_or("").to_string();
            let mut html =
                String::from("<!DOCTYPE html><html><head><meta charset=\"utf-8\"></head><body>");
            html.push_str("<style>a.redlink { color:red }</style>");
            html.push_str(&format!(
                "<h1>{} (<a href='//www.wikidata.org/wiki/{}'>{}</a>)</h1>",
                html_escape::encode_text(&label),
                html_escape::encode_text(&q),
                html_escape::encode_text(&q)
            ));
            if args.links == "wiki" {
                html.push_str(&format!(
                    "<pre style='white-space:pre-wrap;font-size:11pt'>{}</pre>",
                    result
                ));
            } else {
                html.push_str(&format!("<p>{}</p>", result));
            }
            html.push_str("<hr/><div style='font-size:8pt;'>This text was generated automatically from Wikidata using <a href='/'>AutoDesc</a>.</div>");
            html.push_str("</body></html>");
            Html(html).into_response()
        }
        "jsonfm" => {
            let json_text = serde_json::to_string_pretty(
                &serde_json::from_str::<Value>(&cached_json).unwrap_or_default(),
            )
            .unwrap_or(cached_json);
            let mut html =
                String::from("<!DOCTYPE html><html><head><meta charset=\"utf-8\"></head><body>");
            html.push_str("<p>You are looking at the HTML representation of the JSON format. HTML is good for debugging, but is unsuitable for application use.</p>");
            html.push_str("<hr/><pre style='white-space:pre-wrap'>");
            html.push_str(&html_escape::encode_text(&json_text));
            html.push_str("</pre></body></html>");
            Html(html).into_response()
        }
        _ => {
            if let Some(ref callback) = args.callback
                && !callback.is_empty()
            {
                let jsonp = format!("{}({})", callback, cached_json);
                return (
                    StatusCode::OK,
                    [("content-type", "application/javascript; charset=utf-8")],
                    jsonp,
                )
                    .into_response();
            }
            (
                StatusCode::OK,
                [("content-type", "application/json; charset=utf-8")],
                cached_json,
            )
                .into_response()
        }
    }
}

async fn api_handler(State(state): State<AppState>, Query(params): Query<ApiParams>) -> Response {
    let mut args = params.clone();

    // Normalize language
    if args.lang == "any" || args.lang.is_empty() {
        args.lang = DEFAULT_LANGUAGE.to_string();
    }

    // If no Q is provided, return the index HTML
    let q_raw = match &args.q {
        Some(q) if !q.is_empty() => q.clone(),
        _ => return Html(INDEX_HTML.to_string()).into_response(),
    };

    // Normalize Q-id (handles pure digits, mixed case, leading/trailing whitespace).
    let q = sanitize_q(&q_raw);
    args.q = Some(q.clone());

    // Build output cache key from all params that affect the result.
    let output_key = format!(
        "{}|{}|{}|{}|{}|{}|{}|{}|{}|{}",
        q,
        args.lang,
        args.mode,
        args.links,
        args.redlinks,
        args.get_infobox,
        args.infobox_template,
        args.media,
        args.thumb,
        args.user_zoom,
    );

    // Return cached output if available.
    if let Some(cached) = state.output_cache.get(&output_key).await {
        return cached_response(cached, &args);
    }

    // Build description options
    let mut opt = DescOptions {
        q: q.clone(),
        lang: args.lang.clone(),
        links: args.links.clone(),
        mode: args.mode.clone(),
        ..Default::default()
    };

    // Create a WikiData client backed by the shared item cache.
    let mut wd = WikiData::new().with_item_cache(state.item_cache.clone());
    let sd = ShortDescription::new();

    // Generate description
    let (_result_q, output) = sd.load_item(&q, &mut opt, &mut wd).await;

    // Get label and manual description
    let label = wd
        .get_item(&q)
        .map(|i| i.get_label(Some(&args.lang)))
        .unwrap_or_default();

    let manual_desc = wd
        .get_item(&q)
        .map(|i| i.get_desc(Some(&args.lang)))
        .unwrap_or_default();

    // Build the call object for the response
    let call = json!({
        "q": args.q,
        "lang": args.lang,
        "mode": args.mode,
        "links": args.links,
        "redlinks": args.redlinks,
        "format": args.format,
        "get_infobox": args.get_infobox,
        "infobox_template": args.infobox_template,
        "media": args.media,
        "thumb": args.thumb,
        "user_zoom": args.user_zoom,
        "callback": args.callback,
    });

    let mut response = ApiResponse {
        call,
        q: q.clone(),
        label: label.clone(),
        manual_description: manual_desc,
        result: output,
        media: None,
        thumbnails: None,
    };

    add_media(&args, &q, wd, &mut response).await;

    // Serialize the response and store in the output cache, but skip caching
    // error results so they can be retried on the next request.
    let cached_json = serde_json::to_string(&response).unwrap_or_default();
    let cannot_describe = format!("<i>{}</i>", sd.txt("cannot_describe", &args.lang));
    if response.result != cannot_describe {
        state.output_cache.insert(output_key, cached_json).await;
    }

    // Format the response
    match args.format.as_str() {
        "html" => render_html(&args, q, label, &response),
        "jsonfm" => render_jsonfm(&args, &response),
        _ => render_json(args, response),
    }
}

async fn add_media(args: &ApiParams, q: &str, mut wd: WikiData, response: &mut ApiResponse) {
    // Handle media generation
    if args.media == "1" {
        let media_result =
            MediaGenerator::generate_media(q, &args.thumb, args.user_zoom, &mut wd).await;

        // Build media JSON (without thumbnails)
        let mut media_json: HashMap<String, Value> = HashMap::new();
        for (key, val) in &media_result.media {
            media_json.insert(key.clone(), val.clone());
        }
        response.media = Some(serde_json::to_value(&media_json).unwrap_or_default());

        // Build thumbnails JSON
        if !media_result.thumbnails.is_empty() {
            response.thumbnails =
                Some(serde_json::to_value(&media_result.thumbnails).unwrap_or_default());
        }
    }
}

fn render_json(args: ApiParams, response: ApiResponse) -> axum::http::Response<axum::body::Body> {
    // "json" or anything else (default)
    let json_str = serde_json::to_string(&response).unwrap_or_default();

    // Support JSONP callback
    if let Some(ref callback) = args.callback
        && !callback.is_empty()
    {
        let jsonp = format!("{}({})", callback, json_str);
        return (
            StatusCode::OK,
            [("content-type", "application/javascript; charset=utf-8")],
            jsonp,
        )
            .into_response();
    }

    (
        StatusCode::OK,
        [("content-type", "application/json; charset=utf-8")],
        json_str,
    )
        .into_response()
}

fn render_jsonfm(
    args: &ApiParams,
    response: &ApiResponse,
) -> axum::http::Response<axum::body::Body> {
    // Pretty-printed JSON in an HTML wrapper
    let json_text = serde_json::to_string_pretty(response).unwrap_or_default();

    // Build a link to the JSON version
    let mut json_params: Vec<String> = Vec::new();
    if let Some(ref q_val) = args.q {
        json_params.push(format!("q={}", html_escape::encode_text(q_val)));
    }
    json_params.push(format!("lang={}", html_escape::encode_text(&args.lang)));
    json_params.push(format!("mode={}", html_escape::encode_text(&args.mode)));
    json_params.push(format!("links={}", html_escape::encode_text(&args.links)));
    json_params.push("format=json".to_string());
    if !args.media.is_empty() {
        json_params.push(format!("media={}", html_escape::encode_text(&args.media)));
    }
    if !args.thumb.is_empty() {
        json_params.push(format!("thumb={}", html_escape::encode_text(&args.thumb)));
    }

    let json_link = format!("<a href='?{}'>format=json</a>", json_params.join("&"));

    let mut html = String::from("<!DOCTYPE html><html><head><meta charset=\"utf-8\"></head><body>");
    html.push_str("<p>You are looking at the HTML representation of the JSON format. HTML is good for debugging, but is unsuitable for application use.</p>");
    html.push_str(&format!(
        "<p>Specify the format parameter to change the output format. To see the non-HTML representation of the JSON format, set {}.</p>",
        json_link
    ));
    html.push_str("<hr/><pre style='white-space:pre-wrap'>");
    html.push_str(&html_escape::encode_text(&json_text));
    html.push_str("</pre></body></html>");
    Html(html).into_response()
}

fn render_html(
    args: &ApiParams,
    q: String,
    label: String,
    response: &ApiResponse,
) -> axum::http::Response<axum::body::Body> {
    let mut html = String::from("<!DOCTYPE html><html><head><meta charset=\"utf-8\"></head><body>");
    html.push_str("<style>a.redlink { color:red }</style>");
    html.push_str(&format!(
        "<h1>{} (<a href='//www.wikidata.org/wiki/{}'>{}</a>)</h1>",
        html_escape::encode_text(&label),
        html_escape::encode_text(&q),
        html_escape::encode_text(&q)
    ));
    if args.links == "wiki" {
        html.push_str(&format!(
            "<pre style='white-space:pre-wrap;font-size:11pt'>{}</pre>",
            response.result
        ));
    } else {
        html.push_str(&format!("<p>{}</p>", response.result));
    }
    html.push_str("<hr/><div style='font-size:8pt;'>This text was generated automatically from Wikidata using <a href='/'>AutoDesc</a>.</div>");
    html.push_str("</body></html>");
    Html(html).into_response()
}

#[tokio::main]
async fn main() {
    // Initialize tracing
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .init();

    let state = AppState::new();

    let max_concurrency = std::env::var("AUTODESC_MAX_CONCURRENCY")
        .ok()
        .and_then(|v| v.parse::<usize>().ok())
        .unwrap_or(usize::MAX);

    tracing::info!(max_concurrency, "Concurrency limit");

    let timeout_sec = std::env::var("AUTODESC_TIMEOUT_SEC")
        .ok()
        .and_then(|v| v.parse::<u64>().ok())
        .unwrap_or(u64::MAX);

    tracing::info!(timeout_sec, "Timeout limit");

    let app = Router::new()
        .route("/", get(api_handler))
        .with_state(state)
        .layer(CompressionLayer::new())
        .layer(CorsLayer::permissive())
        .layer(TimeoutLayer::new(Duration::from_secs(timeout_sec)))
        .layer(
            ServiceBuilder::new()
                .layer(HandleErrorLayer::new(|_: BoxError| async {
                    StatusCode::SERVICE_UNAVAILABLE
                }))
                .load_shed()
                .concurrency_limit(max_concurrency),
        );

    let address = std::env::var("AUTODESC_ADDRESS")
        .ok()
        .unwrap_or("0.0.0.0".to_string());

    let port: u16 = std::env::var("AUTODESC_PORT")
        .ok()
        .and_then(|v| v.parse::<u16>().ok())
        .unwrap_or(8000);

    let bind_addr = format!("{address}:{port}");
    tracing::info!("AutoDesc server starting on http://{}", bind_addr);

    let listener = tokio::net::TcpListener::bind(&bind_addr)
        .await
        .expect("Failed to bind address");

    axum::serve(listener, app).await.expect("Server error");
}
