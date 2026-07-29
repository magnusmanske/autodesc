# Changelog

All notable changes to the AutoDesc project.

## [0.3.0] — 2026-07-29

### Added

- **`QId` type** (`src/qid.rs`) — validated, normalized Wikidata identifier.
  Replaces the free functions `sanitize_q` and `unified_id`.  Once parsed, a
  `QId` is guaranteed valid; the compiler prevents un-normalized strings from
  reaching cache lookups or the Wikidata API.  Implements "parse, don't
  validate".
- **`Format` enum** (`src/format_type.rs`) — `Json | JsonFm | Html`.  Invalid
  formats are rejected by serde at deserialization time, removing the need for
  runtime `validate_format()` checks.
- **`Lang` type** (`src/lang_type.rs`) — validated language-code newtype.  The
  set of valid ISO 639-1 codes is fetched at startup from Wikidata's SPARQL
  endpoint and cached; falls back to a ~300-code hardcoded list if the query
  fails.
- **`/health` endpoint** — returns `{"status":"ok","uptime_secs":N}`.
- **Per-IP rate limiting** (configurable via `AUTODESC_RATE_LIMIT` /
  `AUTODESC_RATE_WINDOW_SECS`).  Returns HTTP 429 when exceeded.
- **Graceful shutdown** — handles SIGTERM/SIGINT, drains in-flight requests.
- **Security headers**: `X-Content-Type-Options`, `X-Frame-Options`,
  `X-XSS-Protection`, `Referrer-Policy` on every response.
- **Request body size limit** (8 KiB, defense-in-depth).
- **JSONP callback validation** — rejects non-JS-identifier callbacks,
  preventing XSS via callback injection.
- **Input validation module** (`src/validation.rs`) — strict allowlists for
  `lang`, `mode`, `links`, `format`, plus Q-id format and parameter-length
  checks.
- **Exponential-backoff retry** on all Wikidata API calls (up to 3 attempts).
- Env vars: `AUTODESC_TIMEOUT_SEC`, `AUTODESC_SEMAPHORE_TIMEOUT_SECS`,
  `AUTODESC_REQWEST_POOL_IDLE`, `AUTODESC_RATE_LIMIT`,
  `AUTODESC_RATE_WINDOW_SECS`.

### Changed

- **Wikidata API semaphore now uses a timeout** instead of blocking
  indefinitely.  When overwhelmed, requests fail fast (returning an error)
  rather than piling up and eventually producing 503s.
- **Reqwest connection pool** increased from 32 → 128 idle connections per
  host, with keep-alive (60 s) and idle timeout (90 s).
- **CORS** restricted from permissive to Wikimedia origins only.
- `DescOptions.q` changed from `String` to `QId`.
- `DescOptions.lang` changed from `String` to `Lang`.
- `ApiParams.format` changed from `String` to `Format` enum.
- `WikiData.items` and both moka caches now keyed by `QId` instead of raw
  `String`.
- Error responses are now structured JSON (`{"error": "…", "errors": […]}`)
  instead of bare HTTP status codes.

### Removed

- `sanitize_q` and `unified_id` free functions (replaced by `QId::parse` /
  `QId::from_api_key`).
- `validate_format` runtime check (replaced by serde deserialization into
  the `Format` enum).
- Hardcoded `ALLOWED_LANGS` array (replaced by SPARQL-backed `Lang` type with
  hardcoded fallback).

### Fixed

- **Concurrency bottleneck**: the Wikidata API semaphore (500 permits) was far
  smaller than the tower concurrency limit (5 000), causing thousands of
  requests to block and eventually 503.  The semaphore now times out quickly
  instead of blocking forever.
- **JSONP XSS vector**: unvalidated `callback` parameter could inject arbitrary
  JavaScript.
