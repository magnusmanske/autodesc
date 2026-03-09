use std::collections::HashMap;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::wikidata::{sanitize_q, WikiData};

/// Media properties mapped from Wikidata property IDs to human-readable names.
const MEDIA_PROPS: &[(&str, &str)] = &[
    ("P18", "image"),
    ("P94", "coat_of_arms"),
    ("P158", "seal"),
    ("P41", "flag"),
    ("P10", "video"),
    ("P242", "map"),
    ("P948", "banner"),
    ("P154", "logo"),
];

/// A single thumbnail entry.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ThumbnailInfo {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub thumburl: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub thumbwidth: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub thumbheight: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub descriptionurl: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub descriptionshorturl: Option<String>,
}

/// The result of media generation for an item.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct MediaResult {
    /// Mapping from media type name (e.g. "image", "logo") to list of filenames.
    #[serde(flatten)]
    pub media: HashMap<String, Value>,

    /// Thumbnail info keyed by filename.
    #[serde(skip)]
    pub thumbnails: HashMap<String, ThumbnailInfo>,
}

/// Generates media information for a Wikidata item.
pub struct MediaGenerator;

impl MediaGenerator {
    /// Generate media data for the given Q-id.
    ///
    /// - `thumb`: thumbnail size as a string (e.g. "80"). Empty string or "0" means no thumbnails.
    /// - `user_zoom`: zoom level for OSM map thumbnails (default 4).
    pub async fn generate_media(
        q: &str,
        thumb: &str,
        user_zoom: u32,
        wd: &mut WikiData,
    ) -> MediaResult {
        let mut result = MediaResult::default();
        let q = sanitize_q(q);

        if let Err(e) = wd.load_entity(&q).await {
            tracing::warn!("Failed to load entity {} for media: {}", q, e);
            return result;
        }

        let item = match wd.get_item(&q) {
            Some(i) => i.clone(),
            None => return result,
        };

        let mut files: Vec<String> = Vec::new();

        // Extract media files from known properties
        for (prop_id, prop_label) in MEDIA_PROPS {
            if item.has_claims(prop_id) {
                let strings = item.get_strings_for_property(prop_id);
                let cleaned: Vec<String> = strings.iter().map(|s| s.replace('_', " ")).collect();

                for filename in &cleaned {
                    files.push(format!("File:{}", filename));
                }

                result.media.insert(
                    prop_label.to_string(),
                    serde_json::to_value(&cleaned).unwrap_or_default(),
                );
            }
        }

        // OSM map thumbnail for items with coordinates (P625)
        if item.has_claims("P625") {
            let claims = item.get_claims_for_property("P625");
            if let Some(first) = claims.first() {
                let lat = first
                    .get("mainsnak")
                    .and_then(|ms| ms.get("datavalue"))
                    .and_then(|dv| dv.get("value"))
                    .and_then(|v| v.get("latitude"))
                    .and_then(|l| l.as_f64());
                let lon = first
                    .get("mainsnak")
                    .and_then(|ms| ms.get("datavalue"))
                    .and_then(|dv| dv.get("value"))
                    .and_then(|v| v.get("longitude"))
                    .and_then(|l| l.as_f64());

                if let (Some(lat), Some(lon)) = (lat, lon) {
                    result.media.insert(
                        "osm".to_string(),
                        serde_json::to_value(vec!["osm_map"]).unwrap_or_default(),
                    );

                    let zoom = user_zoom;
                    let thumb_size = thumb.parse::<u64>().unwrap_or(0);
                    let thumburl = format!(
                        "https://maps.wikimedia.org/img/osm-intl,{},{},{},{}x{}.png",
                        zoom, lat, lon, thumb_size, thumb_size
                    );

                    result.thumbnails.insert(
                        "osm_map".to_string(),
                        ThumbnailInfo {
                            thumburl: Some(thumburl.clone()),
                            thumbwidth: Some(thumb_size),
                            thumbheight: None,
                            url: Some(thumburl),
                            descriptionurl: Some(format!(
                                "https://tools.wmflabs.org/geohack/geohack.php?language=en&params={}_N_{}_E_globe:earth",
                                lat, lon
                            )),
                            descriptionshorturl: None,
                        },
                    );
                }
            }
        }

        // Fetch thumbnails from Commons API if thumb size is specified
        if !files.is_empty() {
            let thumb_size: u64 = match thumb.parse() {
                Ok(v) if v > 0 => v,
                _ => return result,
            };

            let commons_api = "https://commons.wikimedia.org/w/api.php";
            let titles = files.join("|");
            let thumb_str = thumb_size.to_string();

            let params = [
                ("action", "query"),
                ("titles", &titles),
                ("prop", "imageinfo"),
                ("iiprop", "url"),
                ("iiurlwidth", &thumb_str),
                ("iiurlheight", &thumb_str),
                ("format", "json"),
            ];

            match wd.post_json(commons_api, &params).await {
                Ok(data) => {
                    if let Some(pages) = data
                        .get("query")
                        .and_then(|q| q.get("pages"))
                        .and_then(|p| p.as_object())
                    {
                        for (_page_id, page_val) in pages {
                            let title =
                                page_val.get("title").and_then(|t| t.as_str()).unwrap_or("");

                            // Remove "File:" prefix and normalize underscores to spaces
                            let file_key = title
                                .strip_prefix("File:")
                                .unwrap_or(title)
                                .replace('_', " ");

                            if let Some(imageinfo) = page_val
                                .get("imageinfo")
                                .and_then(|ii| ii.as_array())
                                .and_then(|arr| arr.first())
                            {
                                let info = ThumbnailInfo {
                                    thumburl: imageinfo
                                        .get("thumburl")
                                        .and_then(|v| v.as_str())
                                        .map(String::from),
                                    thumbwidth: imageinfo
                                        .get("thumbwidth")
                                        .and_then(|v| v.as_u64()),
                                    thumbheight: imageinfo
                                        .get("thumbheight")
                                        .and_then(|v| v.as_u64()),
                                    url: imageinfo
                                        .get("url")
                                        .and_then(|v| v.as_str())
                                        .map(String::from),
                                    descriptionurl: imageinfo
                                        .get("descriptionurl")
                                        .and_then(|v| v.as_str())
                                        .map(String::from),
                                    descriptionshorturl: imageinfo
                                        .get("descriptionshorturl")
                                        .and_then(|v| v.as_str())
                                        .map(String::from),
                                };
                                result.thumbnails.insert(file_key, info);
                            }
                        }
                    }
                }
                Err(e) => {
                    tracing::warn!("Failed to fetch thumbnails from Commons: {}", e);
                }
            }
        }

        result
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test(flavor = "multi_thread")]
    async fn test_generate_media_nonexistent() {
        let mut wd = WikiData::new();
        let result = MediaGenerator::generate_media("Q0", "", 4, &mut wd).await;
        assert!(result.media.is_empty());
        assert!(result.thumbnails.is_empty());
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn test_generate_media_with_image() {
        let mut wd = WikiData::new();
        // Q350 is Cambridge - has image, coat of arms, banner, and coordinates
        let result = MediaGenerator::generate_media("Q350", "80", 4, &mut wd).await;
        assert!(
            result.media.contains_key("image") || result.media.contains_key("coat_of_arms"),
            "Cambridge should have image or coat_of_arms, got keys: {:?}",
            result.media.keys().collect::<Vec<_>>()
        );
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn test_generate_media_no_thumb() {
        let mut wd = WikiData::new();
        // Q350 is Cambridge
        let result = MediaGenerator::generate_media("Q350", "", 4, &mut wd).await;
        // Should still have media entries but potentially no thumbnails from Commons
        // (OSM thumbnail may still be present if coordinates exist)
        assert!(
            result.media.contains_key("image")
                || result.media.contains_key("coat_of_arms")
                || result.media.contains_key("osm"),
            "Cambridge should have some media entries"
        );
    }
}
