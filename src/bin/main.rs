use std::collections::HashMap;

use axum::{
    extract::Query,
    http::StatusCode,
    response::{Html, IntoResponse, Response},
    routing::get,
    Router,
};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use tower_http::cors::CorsLayer;

use autodesc::desc_options::DescOptions;
use autodesc::media::MediaGenerator;
use autodesc::short_desc::ShortDescription;
use autodesc::wikidata::{sanitize_q, WikiData};

const DEFAULT_LANGUAGE: &str = "en";

const INDEX_HTML: &str = r#"<!DOCTYPE html>
<html>
<head>
<meta charset="utf-8">
<title>AutoDesc - Wikidata automated description API</title>
<style>
body { font-family: sans-serif; max-width: 800px; margin: 40px auto; padding: 0 20px; }
h1 { color: #006699; }
form { background: #f5f5f5; padding: 20px; border-radius: 8px; margin: 20px 0; }
label { display: inline-block; width: 120px; font-weight: bold; }
input, select { margin: 4px 0; }
.examples { margin-top: 20px; }
.examples a { display: block; margin: 4px 0; }
</style>
</head>
<body>
<h1>AutoDesc - Wikidata automated description API</h1>
<p>This API can describe most Wikidata items automatically. For some item types (e.g. biographies) in some languages, long descriptions are available.</p>
<form method="get" action="/">
<div><label>Q</label> <input type="text" name="q" placeholder="Q42" size="10"></div>
<div><label>Language</label> <input type="text" name="lang" value="en" size="5"></div>
<div><label>Mode</label>
  <input type="radio" name="mode" value="short" checked> short
  <input type="radio" name="mode" value="long"> long description (where possible)
</div>
<div><label>Links</label>
  <select name="links">
    <option value="text">plain text</option>
    <option value="wikidata">HTML/Wikidata</option>
    <option value="wikipedia">HTML/Wikipedia</option>
    <option value="wiki">Wikitext/Wikipedia</option>
    <option value="reasonator">HTML/Reasonator</option>
  </select>
</div>
<div><label>Format</label>
  <select name="format">
    <option value="jsonfm">Pretty JSON</option>
    <option value="json">JSON (takes &amp;callback)</option>
    <option value="html">HTML page</option>
  </select>
</div>
<div><label>Media</label>
  <input type="checkbox" name="media" value="1"> Include media info
</div>
<div style="margin-top: 10px;"><input type="submit" value="Generate"></div>
</form>
<div class="examples">
<h3>Examples:</h3>
<a href="/?q=Q1339&lang=en&mode=long&links=wikipedia&format=html&redlinks=reasonator">Long English description of J.S.Bach, as a HTML page linking to English Wikipedia</a>
<a href="/?q=Q42&lang=en&mode=short&links=text&format=json">Douglas Adams, short description, plain text, JSON</a>
<a href="/?q=Q12345&lang=en&links=wikidata&format=json">Count von Count, wikidata links, JSON</a>
</div>
</body>
</html>"#;

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

async fn api_handler(Query(params): Query<ApiParams>) -> Response {
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

    // Create a fresh WikiData client per request
    let mut wd = WikiData::new();
    let sd = ShortDescription::new();

    // Build description options
    let mut opt = DescOptions {
        q: q.clone(),
        lang: args.lang.clone(),
        links: args.links.clone(),
        mode: args.mode.clone(),
        ..Default::default()
    };

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

    // Handle media generation
    if args.media == "1" {
        let media_result =
            MediaGenerator::generate_media(&q, &args.thumb, args.user_zoom, &mut wd).await;

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

    // Format the response
    match args.format.as_str() {
        "html" => {
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
                    response.result
                ));
            } else {
                html.push_str(&format!("<p>{}</p>", response.result));
            }
            html.push_str("<hr/><div style='font-size:8pt;'>This text was generated automatically from Wikidata using <a href='/'>AutoDesc</a>.</div>");
            html.push_str("</body></html>");
            Html(html).into_response()
        }
        "jsonfm" => {
            // Pretty-printed JSON in an HTML wrapper
            let json_text = serde_json::to_string_pretty(&response).unwrap_or_default();

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

            let mut html =
                String::from("<!DOCTYPE html><html><head><meta charset=\"utf-8\"></head><body>");
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
        _ => {
            // "json" or anything else (default)
            let json_str = serde_json::to_string(&response).unwrap_or_default();

            // Support JSONP callback
            if let Some(ref callback) = args.callback {
                if !callback.is_empty() {
                    let jsonp = format!("{}({})", callback, json_str);
                    return (
                        StatusCode::OK,
                        [("content-type", "application/javascript; charset=utf-8")],
                        jsonp,
                    )
                        .into_response();
                }
            }

            (
                StatusCode::OK,
                [("content-type", "application/json; charset=utf-8")],
                json_str,
            )
                .into_response()
        }
    }
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

    let app = Router::new()
        .route("/", get(api_handler))
        .layer(CorsLayer::permissive());

    let port: u16 = std::env::var("AUTODESC_PORT")
        .ok()
        .and_then(|v| v.parse::<u16>().ok())
        .unwrap_or(8000);

    let bind_addr = format!("0.0.0.0:{port}");
    tracing::info!("AutoDesc server starting on http://{}", bind_addr);

    let listener = tokio::net::TcpListener::bind(&bind_addr)
        .await
        .expect("Failed to bind address");

    axum::serve(listener, app).await.expect("Server error");
}
