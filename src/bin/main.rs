use autodesc::Format;
use autodesc::Lang;
use autodesc::desc_options::DescOptions;
use autodesc::lang_type;
use autodesc::media::MediaGenerator;
use autodesc::qid::QId;
use autodesc::short_desc::ShortDescription;
use autodesc::validation;
use autodesc::wikidata::{WikiData, WikiDataItem};
use axum::{
    Router,
    error_handling::HandleErrorLayer,
    extract::{ConnectInfo, Query, State},
    http::{HeaderValue, StatusCode, header},
    response::{Html, IntoResponse, Json, Response},
    routing::get,
};
use moka::future::Cache;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::RwLock;
use tower::BoxError;
use tower::ServiceBuilder;
use tower_http::compression::CompressionLayer;
use tower_http::cors::CorsLayer;
use tower_http::limit::RequestBodyLimitLayer;
use tower_http::set_header::response::SetResponseHeaderLayer;
use tower_http::timeout::TimeoutLayer;

#[global_allocator]
static GLOBAL: mimalloc::MiMalloc = mimalloc::MiMalloc;

// ── Rate limiting ──────────────────────────────────────────────────────────

/// Per-IP rate limiter using a simple sliding-window approach.
#[derive(Clone)]
struct IpRateLimiter {
    buckets: Arc<RwLock<std::collections::HashMap<std::net::IpAddr, (Instant, u64)>>>,
    max_requests: u64,
    window: Duration,
}

impl IpRateLimiter {
    fn new(max_requests: u64, window_secs: u64) -> Self {
        Self {
            buckets: Arc::new(RwLock::new(std::collections::HashMap::new())),
            max_requests,
            window: Duration::from_secs(window_secs),
        }
    }

    /// Returns true if the request is allowed, false if rate-limited.
    async fn check(&self, ip: std::net::IpAddr) -> bool {
        let now = Instant::now();
        let mut buckets = self.buckets.write().await;

        let entry = buckets.entry(ip).or_insert((now, 0));

        if now.duration_since(entry.0) > self.window {
            entry.0 = now;
            entry.1 = 0;
        }

        if entry.1 >= self.max_requests {
            false
        } else {
            entry.1 += 1;
            true
        }
    }
}

// ── App state ──────────────────────────────────────────────────────────────

#[derive(Clone)]
struct AppState {
    item_cache: Cache<QId, WikiDataItem>,
    output_cache: Cache<String, String>,
    rate_limiter: IpRateLimiter,
    start_time: Instant,
}

impl AppState {
    fn new() -> Self {
        let item_ttl = std::env::var("AUTODESC_ITEM_CACHE_TTL_SECS")
            .ok()
            .and_then(|v| v.parse::<u64>().ok())
            .unwrap_or(3600);
        let item_size = std::env::var("AUTODESC_ITEM_CACHE_SIZE")
            .ok()
            .and_then(|v| v.parse::<u64>().ok())
            .unwrap_or(10_000);

        let output_ttl = std::env::var("AUTODESC_OUTPUT_CACHE_TTL_SECS")
            .ok()
            .and_then(|v| v.parse::<u64>().ok())
            .unwrap_or(600);
        let output_size = std::env::var("AUTODESC_OUTPUT_CACHE_SIZE")
            .ok()
            .and_then(|v| v.parse::<u64>().ok())
            .unwrap_or(1_000);

        let rate_limit = std::env::var("AUTODESC_RATE_LIMIT")
            .ok()
            .and_then(|v| v.parse::<u64>().ok())
            .unwrap_or(100);
        let rate_window = std::env::var("AUTODESC_RATE_WINDOW_SECS")
            .ok()
            .and_then(|v| v.parse::<u64>().ok())
            .unwrap_or(10);

        tracing::info!(
            item_ttl,
            item_size,
            output_ttl,
            output_size,
            rate_limit,
            rate_window,
            "Cache and rate-limit configuration"
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
            rate_limiter: IpRateLimiter::new(rate_limit, rate_window),
            start_time: Instant::now(),
        }
    }
}

// ── Constants ──────────────────────────────────────────────────────────────

const DEFAULT_LANGUAGE: &str = "en";
const INDEX_HTML: &str = include_str!("../../data/index.html");

// ── Query parameters ───────────────────────────────────────────────────────

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
    pub format: Format,
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
fn default_format() -> Format {
    Format::JsonFm
}
fn default_get_infobox() -> String {
    "yes".to_string()
}
fn default_user_zoom() -> u32 {
    4
}

// ── Validation ─────────────────────────────────────────────────────────────

struct ValidationErrors {
    errors: Vec<String>,
}

impl ValidationErrors {
    fn new() -> Self {
        Self { errors: Vec::new() }
    }
    fn push(&mut self, msg: impl Into<String>) {
        self.errors.push(msg.into());
    }
    fn is_empty(&self) -> bool {
        self.errors.is_empty()
    }
}

fn validate_params(params: &ApiParams) -> Result<(), ValidationErrors> {
    let mut errs = ValidationErrors::new();

    if let Some(ref q) = params.q
        && !q.is_empty()
        && !validation::validate_qid(q)
    {
        errs.push(format!(
            "Invalid Q-id: '{}'. Expected format: Q followed by digits.",
            q
        ));
    }

    if !validation::validate_lang(&params.lang) {
        errs.push(format!("Invalid language: '{}'", params.lang));
    }

    if !validation::validate_mode(&params.mode) {
        errs.push(format!(
            "Invalid mode: '{}'. Allowed: short, long",
            params.mode
        ));
    }

    if !validation::validate_links(&params.links) {
        errs.push(format!(
            "Invalid links: '{}'. Allowed: text, wikidata, wiki, wikipedia, reasonator",
            params.links
        ));
    }

    if let Some(ref callback) = params.callback
        && !callback.is_empty()
        && !validation::validate_jsonp_callback(callback)
    {
        errs.push(format!(
            "Invalid JSONP callback: '{}'. Must be a valid JavaScript identifier.",
            callback
        ));
    }

    for (name, value) in &[
        ("lang", &params.lang),
        ("mode", &params.mode),
        ("links", &params.links),
        ("redlinks", &params.redlinks),
        ("get_infobox", &params.get_infobox),
        ("infobox_template", &params.infobox_template),
        ("media", &params.media),
        ("thumb", &params.thumb),
    ] {
        if !validation::validate_param_length(value) {
            errs.push(format!(
                "Parameter '{}' value too long (max 512 chars)",
                name
            ));
        }
    }

    if !params.thumb.is_empty() {
        if let Ok(t) = params.thumb.parse::<u64>() {
            if t > 4096 {
                errs.push("Thumbnail size must be ≤ 4096px".to_string());
            }
        } else {
            errs.push(format!("Invalid thumbnail size: '{}'", params.thumb));
        }
    }

    if errs.is_empty() { Ok(()) } else { Err(errs) }
}

// ── Response types ─────────────────────────────────────────────────────────

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

// ── Response rendering ─────────────────────────────────────────────────────

fn error_json(status: StatusCode, msg: &str) -> Response {
    let body = json!({ "error": msg });
    (
        status,
        [("content-type", "application/json; charset=utf-8")],
        body.to_string(),
    )
        .into_response()
}

fn error_json_multi(status: StatusCode, msg: &str, errors: &[String]) -> Response {
    let body = json!({ "error": msg, "errors": errors });
    (
        status,
        [("content-type", "application/json; charset=utf-8")],
        body.to_string(),
    )
        .into_response()
}

fn cached_response(cached_json: String, args: &ApiParams) -> Response {
    match args.format {
        Format::Html => {
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
        Format::JsonFm => {
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
        Format::Json => {
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

// ── Handlers ───────────────────────────────────────────────────────────────

async fn health_handler(State(state): State<AppState>) -> Json<Value> {
    let uptime = state.start_time.elapsed().as_secs();
    Json(json!({
        "status": "ok",
        "uptime_secs": uptime,
    }))
}

async fn api_handler(
    State(state): State<AppState>,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    Query(params): Query<ApiParams>,
) -> Response {
    // Rate limiting
    if !state.rate_limiter.check(addr.ip()).await {
        return error_json(
            StatusCode::TOO_MANY_REQUESTS,
            "Rate limit exceeded. Please slow down.",
        );
    }

    // Input validation
    if let Err(validation_errors) = validate_params(&params) {
        return error_json_multi(
            StatusCode::BAD_REQUEST,
            "Invalid request parameters",
            &validation_errors.errors,
        );
    }

    let mut args = params.clone();

    if args.lang == "any" || args.lang.is_empty() {
        args.lang = DEFAULT_LANGUAGE.to_string();
    }

    if args.format == Format::Html && args.media == "1" && args.thumb.is_empty() {
        args.thumb = "200".to_string();
    }

    let q_raw = match &args.q {
        Some(q) if !q.is_empty() => q.clone(),
        _ => return Html(INDEX_HTML.to_string()).into_response(),
    };

    let q = match QId::parse(&q_raw) {
        Ok(q) => q,
        Err(e) => return error_json(StatusCode::BAD_REQUEST, &format!("Invalid Q-id: {e}")),
    };
    args.q = Some(q.as_str().to_string());

    let output_key = format!(
        "{}|{}|{}|{}|{}|{}|{}|{}|{}|{}|{}",
        q.as_str(),
        args.lang,
        args.mode,
        args.links,
        args.redlinks,
        args.format.as_str(),
        args.get_infobox,
        args.infobox_template,
        args.media,
        args.thumb,
        args.user_zoom,
    );

    if let Some(cached) = state.output_cache.get(&output_key).await {
        return cached_response(cached, &args);
    }

    let mut opt = DescOptions {
        q: q.clone(),
        lang: Lang::parse(&args.lang).unwrap_or_else(|_| Lang::default()),
        links: args.links.clone(),
        mode: args.mode.clone(),
        ..Default::default()
    };

    let mut wd = WikiData::new().with_item_cache(state.item_cache.clone());
    let sd = ShortDescription::global();

    let (_result_q, output) = sd.load_item(q.as_str(), &mut opt, &mut wd).await;

    let label = wd
        .get_item(&q)
        .map(|i| i.get_label(Some(&args.lang)))
        .unwrap_or_default();

    let manual_desc = wd
        .get_item(&q)
        .map(|i| i.get_desc(Some(&args.lang)))
        .unwrap_or_default();

    let call = json!({
        "q": args.q,
        "lang": args.lang,
        "mode": args.mode,
        "links": args.links,
        "redlinks": args.redlinks,
        "format": args.format.as_str(),
        "get_infobox": args.get_infobox,
        "infobox_template": args.infobox_template,
        "media": args.media,
        "thumb": args.thumb,
        "user_zoom": args.user_zoom,
        "callback": args.callback,
    });

    let mut response = ApiResponse {
        call,
        q: q.to_string(),
        label: label.clone(),
        manual_description: manual_desc,
        result: output,
        media: None,
        thumbnails: None,
    };

    add_media(&args, q.as_str(), wd, &mut response).await;

    let cached_json = serde_json::to_string(&response).unwrap_or_default();
    let cannot_describe = format!("<i>{}</i>", sd.txt("cannot_describe", &args.lang));
    if response.result != cannot_describe {
        state.output_cache.insert(output_key, cached_json).await;
    }

    match args.format {
        Format::Html => render_html(&args, q.to_string(), label, &response),
        Format::JsonFm => render_jsonfm(&args, &response),
        Format::Json => render_json(args, response),
    }
}

async fn add_media(args: &ApiParams, q: &str, mut wd: WikiData, response: &mut ApiResponse) {
    if args.media == "1" {
        let media_result =
            MediaGenerator::generate_media(q, &args.thumb, args.user_zoom, &mut wd).await;

        if !media_result.media.is_empty() {
            response.media = Some(serde_json::to_value(&media_result.media).unwrap_or_default());
        }

        if !media_result.thumbnails.is_empty() {
            response.thumbnails =
                Some(serde_json::to_value(&media_result.thumbnails).unwrap_or_default());
        }
    }
}

fn render_json(args: ApiParams, response: ApiResponse) -> Response {
    let json_str = serde_json::to_string(&response).unwrap_or_default();

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

fn render_jsonfm(args: &ApiParams, response: &ApiResponse) -> Response {
    let json_text = serde_json::to_string_pretty(response).unwrap_or_default();

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

fn first_thumbnail(response: &ApiResponse) -> Option<(String, String)> {
    let thumbnails = response.thumbnails.as_ref()?.as_object()?;
    let media = response.media.as_ref()?.as_object()?;

    for media_type in &[
        "image",
        "coat_of_arms",
        "logo",
        "flag",
        "seal",
        "banner",
        "map",
        "osm",
    ] {
        if let Some(files) = media.get(*media_type).and_then(|v| v.as_array()) {
            for file in files {
                if let Some(filename) = file.as_str()
                    && let Some(info) = thumbnails.get(filename)
                    && let Some(thumburl) = info.get("thumburl").and_then(|v| v.as_str())
                {
                    let descurl = info
                        .get("descriptionurl")
                        .and_then(|v| v.as_str())
                        .unwrap_or(thumburl);
                    return Some((thumburl.to_string(), descurl.to_string()));
                }
            }
        }
    }
    None
}

fn render_html(args: &ApiParams, q: String, label: String, response: &ApiResponse) -> Response {
    let mut html = String::from("<!DOCTYPE html><html><head><meta charset=\"utf-8\"></head><body>");
    html.push_str("<style>a.redlink { color:red }</style>");
    html.push_str(&format!(
        "<h1>{} (<a href='//www.wikidata.org/wiki/{}'>{}</a>)</h1>",
        html_escape::encode_text(&label),
        html_escape::encode_text(&q),
        html_escape::encode_text(&q)
    ));

    if let Some((thumburl, descurl)) = first_thumbnail(response) {
        html.push_str(&format!(
            "<div style='float:right;margin:0 0 1em 1em;'>\
             <a href='{descurl}' target='_blank'>\
             <img src='{thumburl}' style='max-width:200px;max-height:200px;display:block;'/>\
             </a></div>",
            descurl = html_escape::encode_quoted_attribute(&descurl),
            thumburl = html_escape::encode_quoted_attribute(&thumburl),
        ));
    }

    if args.links == "wiki" {
        html.push_str(&format!(
            "<pre style='white-space:pre-wrap;font-size:11pt'>{}</pre>",
            response.result
        ));
    } else {
        html.push_str(&format!("<p>{}</p>", response.result));
    }
    html.push_str("<div style='clear:both'></div>");
    html.push_str("<hr/><div style='font-size:8pt;'>This text was generated automatically from Wikidata using <a href='/'>AutoDesc</a>.</div>");
    html.push_str("</body></html>");
    Html(html).into_response()
}

// ── Main ───────────────────────────────────────────────────────────────────

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .init();

    lang_type::init_langs().await;

    let state = AppState::new();

    let max_concurrency = std::env::var("AUTODESC_MAX_CONCURRENCY")
        .ok()
        .and_then(|v| v.parse::<usize>().ok())
        .unwrap_or(5000);

    tracing::info!(max_concurrency, "Concurrency limit");

    let timeout_sec = std::env::var("AUTODESC_TIMEOUT_SEC")
        .ok()
        .and_then(|v| v.parse::<u64>().ok())
        .unwrap_or(120);

    tracing::info!(timeout_sec, "Timeout limit");

    // Security headers
    let security_headers = vec![
        (
            header::X_CONTENT_TYPE_OPTIONS,
            HeaderValue::from_static("nosniff"),
        ),
        (header::X_FRAME_OPTIONS, HeaderValue::from_static("DENY")),
        (
            header::X_XSS_PROTECTION,
            HeaderValue::from_static("1; mode=block"),
        ),
        (
            header::REFERRER_POLICY,
            HeaderValue::from_static("strict-origin-when-cross-origin"),
        ),
    ];

    let app = Router::new()
        .route("/", get(api_handler))
        .route("/health", get(health_handler))
        .with_state(state);

    let app = security_headers
        .into_iter()
        .fold(app, |router, (name, value)| {
            router.layer(SetResponseHeaderLayer::if_not_present(name, value))
        });

    let app = app
        .layer(RequestBodyLimitLayer::new(8192))
        .layer(CompressionLayer::new())
        .layer(SetResponseHeaderLayer::if_not_present(
            header::CACHE_CONTROL,
            HeaderValue::from_static("public, max-age=3600"),
        ))
        .layer(build_cors_layer())
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

    axum::serve(
        listener,
        app.into_make_service_with_connect_info::<SocketAddr>(),
    )
    .with_graceful_shutdown(shutdown_signal())
    .await
    .expect("Server error");
}

fn build_cors_layer() -> CorsLayer {
    CorsLayer::permissive()
}

async fn shutdown_signal() {
    let ctrl_c = async {
        tokio::signal::ctrl_c()
            .await
            .expect("failed to install Ctrl+C handler");
    };

    #[cfg(unix)]
    let terminate = async {
        tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
            .expect("failed to install signal handler")
            .recv()
            .await;
    };

    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();

    tokio::select! {
        _ = ctrl_c => {},
        _ = terminate => {},
    }

    tracing::info!("Shutdown signal received, starting graceful shutdown...");
}
