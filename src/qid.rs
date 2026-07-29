use serde::{Deserialize, Serialize};
use std::fmt;

/// A validated and normalized Wikidata item identifier (e.g., "Q42").
///
/// Once parsed, a `QId` is guaranteed to be valid — you never need to
/// re-validate or re-normalize it. This replaces the free functions
/// `sanitize_q` and `unified_id` scattered throughout the codebase.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct QId(String);

impl QId {
    /// Parse and normalize a Q-id from user input.
    ///
    /// Accepts formats like `"Q42"`, `"q42"`, `"42"`, `"  Q42  "`.
    pub fn parse(raw: &str) -> Result<Self, QIdError> {
        let trimmed = raw.trim();
        if trimmed.is_empty() {
            return Err(QIdError::Empty);
        }
        if trimmed.len() > 20 {
            return Err(QIdError::TooLong);
        }

        let normalized = if trimmed.starts_with(|c: char| c.eq_ignore_ascii_case(&'Q')) {
            // Already has Q prefix — uppercase the Q and keep the digits.
            let digits = &trimmed[1..];
            if digits.is_empty() || !digits.chars().all(|c| c.is_ascii_digit()) {
                return Err(QIdError::InvalidFormat);
            }
            format!("Q{digits}")
        } else if trimmed.chars().all(|c| c.is_ascii_digit()) {
            // Numeric-only — prepend Q.
            format!("Q{trimmed}")
        } else {
            return Err(QIdError::InvalidFormat);
        };

        Ok(QId(normalized))
    }

    /// Create a `QId` from an already-normalized key returned by the Wikidata
    /// API.  This performs a cheap format check but no re-normalization.
    pub(crate) fn from_api_key(key: &str) -> Self {
        debug_assert!(
            key.len() >= 2 && key.starts_with('Q') && key[1..].chars().all(|c| c.is_ascii_digit()),
            "API returned non-conforming key: {key}"
        );
        QId(key.to_string())
    }

    /// Return the normalized string representation (e.g. `"Q42"`).
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for QId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl AsRef<str> for QId {
    fn as_ref(&self) -> &str {
        &self.0
    }
}

impl From<QId> for String {
    fn from(q: QId) -> Self {
        q.0
    }
}

#[derive(Debug, thiserror::Error)]
pub enum QIdError {
    #[error("Q-id must not be empty")]
    Empty,
    #[error("Q-id is too long (max 20 characters)")]
    TooLong,
    #[error("Invalid Q-id format. Expected Q followed by digits, or just digits.")]
    InvalidFormat,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_q_prefix() {
        let q = QId::parse("Q42").unwrap();
        assert_eq!(q.as_str(), "Q42");
    }

    #[test]
    fn parse_lowercase_q() {
        let q = QId::parse("q42").unwrap();
        assert_eq!(q.as_str(), "Q42");
    }

    #[test]
    fn parse_digits_only() {
        let q = QId::parse("42").unwrap();
        assert_eq!(q.as_str(), "Q42");
    }

    #[test]
    fn parse_with_whitespace() {
        let q = QId::parse("  Q42  ").unwrap();
        assert_eq!(q.as_str(), "Q42");
    }

    #[test]
    fn parse_empty() {
        assert!(QId::parse("").is_err());
        assert!(QId::parse("   ").is_err());
    }

    #[test]
    fn parse_invalid() {
        assert!(QId::parse("foo").is_err());
        assert!(QId::parse("Q").is_err());
        assert!(QId::parse("Qabc").is_err());
    }

    #[test]
    fn parse_too_long() {
        assert!(QId::parse("Q12345678901234567890").is_err());
    }

    #[test]
    fn from_api_key() {
        let q = QId::from_api_key("Q42");
        assert_eq!(q.as_str(), "Q42");
    }

    #[test]
    fn equality_and_hash() {
        let a = QId::parse("Q42").unwrap();
        let b = QId::parse("42").unwrap();
        assert_eq!(a, b);
        use std::collections::HashSet;
        let mut set = HashSet::new();
        set.insert(a);
        assert!(set.contains(&b));
    }

    #[test]
    fn display() {
        let q = QId::parse("q42").unwrap();
        assert_eq!(q.to_string(), "Q42");
    }

    #[test]
    fn serde_roundtrip() {
        let q = QId::parse("Q42").unwrap();
        let json = serde_json::to_string(&q).unwrap();
        assert_eq!(json, "\"Q42\"");
        let q2: QId = serde_json::from_str(&json).unwrap();
        assert_eq!(q, q2);
    }
}
