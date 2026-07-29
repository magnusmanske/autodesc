use serde::{Deserialize, Serialize};

/// Output format for the API response.
///
/// Replaces raw string comparisons like `args.format == "html"`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Format {
    /// Plain JSON (`application/json`).
    Json,
    /// Pretty-printed JSON in an HTML wrapper (debugging view).
    JsonFm,
    /// Full HTML page with label, description, and optional thumbnail.
    Html,
}

impl Format {
    /// Parse from a query-parameter string.
    pub fn parse(s: &str) -> Result<Self, FormatError> {
        match s {
            "json" => Ok(Self::Json),
            "jsonfm" => Ok(Self::JsonFm),
            "html" => Ok(Self::Html),
            other => Err(FormatError(other.to_string())),
        }
    }

    /// Lowercase string representation.
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Json => "json",
            Self::JsonFm => "jsonfm",
            Self::Html => "html",
        }
    }
}

#[derive(Debug, thiserror::Error)]
#[error("invalid format: {0}. Allowed: json, jsonfm, html")]
pub struct FormatError(pub String);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_valid() {
        assert_eq!(Format::parse("json").unwrap(), Format::Json);
        assert_eq!(Format::parse("jsonfm").unwrap(), Format::JsonFm);
        assert_eq!(Format::parse("html").unwrap(), Format::Html);
    }

    #[test]
    fn parse_invalid() {
        assert!(Format::parse("xml").is_err());
    }

    #[test]
    fn as_str_roundtrip() {
        for s in &["json", "jsonfm", "html"] {
            assert_eq!(Format::parse(s).unwrap().as_str(), *s);
        }
    }
}
