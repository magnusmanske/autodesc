# AutoDesc

Generates human-readable descriptions for [Wikidata](https://www.wikidata.org/) items. It can produce short one-line descriptions (in many languages) or longer prose paragraphs for people (currently English, Dutch, and French). Runs as a small HTTP server with a JSON API.

## Building

```
cargo build --release
```

The binary is `target/release/autodesc-server`.

## Running the server

```
./target/release/autodesc-server
```

By default it listens on `0.0.0.0:8000`. Override with environment variables:

| Variable | Default | Purpose |
|---|---|---|
| `AUTODESC_ADDRESS` | `0.0.0.0` | Bind address |
| `AUTODESC_PORT` | `8000` | Bind port |
| `AUTODESC_ITEM_CACHE_TTL_SECS` | `3600` | How long to cache Wikidata items (seconds) |
| `AUTODESC_ITEM_CACHE_SIZE` | `10000` | Max number of cached items |
| `AUTODESC_OUTPUT_CACHE_TTL_SECS` | `600` | How long to cache generated descriptions (seconds) |
| `AUTODESC_OUTPUT_CACHE_SIZE` | `1000` | Max number of cached output strings |

Logging level is controlled via `RUST_LOG` (e.g. `RUST_LOG=debug`).

## API

All requests go to `GET /`. A request without a `q` parameter returns a brief HTML landing page.

### Parameters

| Parameter | Default | Description |
|---|---|---|
| `q` | — | Wikidata item ID, e.g. `Q42`. Plain numbers are accepted too (`42` → `Q42`). |
| `lang` | `en` | Language code for labels and description text. |
| `mode` | `short` | `short` for a one-liner, `long` for a prose paragraph (falls back to short if the language isn't supported for long descriptions). |
| `links` | `text` | How to render item links — see below. |
| `format` | `jsonfm` | Response format: `json`, `jsonfm` (pretty JSON in HTML), or `html`. |
| `media` | — | Set to `1` to include media URLs (images, flags, logos, etc.) in the response. |
| `thumb` | — | Thumbnail width in pixels (e.g. `80`). Only used when `media=1`. |
| `user_zoom` | `4` | OpenStreetMap zoom level for map thumbnails. |
| `callback` | — | JSONP callback function name. |

### Link modes

| Mode | Output |
|---|---|
| `text` | Plain text, no links. |
| `wikidata` | HTML anchor tags pointing to `wikidata.org`. |
| `wiki` | Wikitext-style `[[Page\|Label]]` links. |
| `wikipedia` | HTML anchor tags pointing to the Wikipedia article in the requested language. |
| `reasonator` | Links to [Reasonator](https://reasonator.toolforge.org/). |

### Response fields

```json
{
  "q": "Q42",
  "label": "Douglas Adams",
  "manual_description": "English writer and humourist",
  "result": "♂ British novelist, screenwriter (1952–2001)",
  "call": { "...": "echo of all request parameters" }
}
```

When `media=1`, two extra fields appear: `media` (a map of type → filenames) and `thumbnails` (a map of filename → thumbnail URLs and dimensions).

### Examples

Short description, plain text:
```
GET /?q=Q42&lang=en&links=text
```

Long description, wikitext links:
```
GET /?q=Q42&lang=en&mode=long&links=wiki&format=json
```

Description in Dutch with Wikipedia links:
```
GET /?q=Q42&lang=nl&links=wikipedia&format=json
```

## Long descriptions

Long prose descriptions are generated for **people** only, currently in three languages:

- **English** (`en`)
- **Dutch** (`nl`)
- **French** (`fr`)

They include, where available: nationality, occupation, birth and death details, education, career positions, family, and burial place. If a language doesn't support long descriptions, the response falls back to the short format.

## Using as a library

`autodesc` is also a regular Rust crate. The main types you'll care about:

```rust
use autodesc::desc_options::DescOptions;
use autodesc::short_desc::ShortDescription;
use autodesc::wikidata::WikiData;

let sd = ShortDescription::new();
let mut wd = WikiData::new();
let mut opt = DescOptions {
    lang: "en".to_string(),
    links: "text".to_string(),
    mode: "short".to_string(),
    ..Default::default()
};

let (q, description) = sd.load_item("Q42", &mut opt, &mut wd).await;
println!("{}: {}", q, description);
```

For long descriptions directly:

```rust
use autodesc::long_desc::LongDescGenerator;

let result = LongDescGenerator::generate(&sd, "Q42", &claims, &opt, &mut wd).await;
```

`WikiData` fetches items from the Wikidata API and caches them locally. You can optionally attach a shared [`moka`](https://docs.rs/moka) cache to share items across multiple requests:

```rust
use moka::future::Cache;

let shared_cache = Cache::builder().max_capacity(10_000).build();
let wd = WikiData::new().with_item_cache(shared_cache);
```

## Running the tests

```
cargo test
```

The unit and integration tests are split into two groups. Tests that hit the real Wikidata API live in `tests/api_integration.rs`; tests for the long-description generator use [wiremock](https://docs.rs/wiremock) and don't make any real network requests (`tests/long_desc_tests.rs`).
