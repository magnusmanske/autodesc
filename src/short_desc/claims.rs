use super::ShortDescription;

impl ShortDescription {
    /// Check if claims have a specific P/Q link (both numeric).
    /// Handles both the newer `"id": "Q<n>"` format and the older `"numeric-id": <n>` format.
    pub(super) fn has_pq(claims: &serde_json::Value, p: u64, q: u64) -> bool {
        let prop = format!("P{}", p);
        let claims_arr = match claims.get(&prop).and_then(|v| v.as_array()) {
            Some(arr) => arr,
            None => return false,
        };

        let q_str = format!("Q{}", q);

        for v in claims_arr {
            let value = match v
                .get("mainsnak")
                .and_then(|ms| ms.get("datavalue"))
                .and_then(|dv| dv.get("value"))
            {
                Some(val) => val,
                None => continue,
            };

            if value
                .get("id")
                .and_then(|i| i.as_str())
                .map(|id| id == q_str)
                .unwrap_or(false)
            {
                return true;
            }

            if value
                .get("numeric-id")
                .and_then(|n| n.as_u64())
                .map(|n| n == q)
                .unwrap_or(false)
            {
                return true;
            }
        }
        false
    }

    pub(super) fn is_person(claims: &serde_json::Value) -> bool {
        Self::has_pq(claims, 107, 215627) || Self::has_pq(claims, 31, 5)
    }

    pub(super) fn is_taxon(claims: &serde_json::Value) -> bool {
        Self::has_pq(claims, 31, 16521)
            || Self::has_pq(claims, 105, 7432)
            || Self::has_pq(claims, 105, 34740)
            || Self::has_pq(claims, 105, 35409)
    }

    pub(super) fn is_disambig(claims: &serde_json::Value) -> bool {
        Self::has_pq(claims, 107, 11651459)
    }

    /// Public version of `has_pq` for use by other modules (e.g. long_desc).
    pub fn has_pq_public(claims: &serde_json::Value, p: u64, q: u64) -> bool {
        Self::has_pq(claims, p, q)
    }

    /// Public version of `is_person` for use by other modules (e.g. long_desc).
    pub fn is_person_public(claims: &serde_json::Value) -> bool {
        Self::is_person(claims)
    }

    /// Extract items from claims for a given (numeric) property. Returns [(prop_num, qid), ...].
    pub(super) fn add_items_from_claims(
        claims: &serde_json::Value,
        p: u64,
        items: &mut Vec<(u64, String)>,
    ) {
        let prefixed = format!("P{}", p);
        let claims_arr = match claims.get(&prefixed).and_then(|v| v.as_array()) {
            Some(arr) => arr,
            None => return,
        };

        for v in claims_arr {
            let mainsnak = match v.get("mainsnak") {
                Some(ms) => ms,
                None => continue,
            };
            let datavalue = match mainsnak.get("datavalue") {
                Some(dv) => dv,
                None => continue,
            };
            let value = match datavalue.get("value") {
                Some(val) => val,
                None => continue,
            };

            if let Some(id) = value.get("id").and_then(|i| i.as_str()) {
                items.push((p, id.to_string()));
            } else if let Some(numeric_id) = value.get("numeric-id").and_then(|n| n.as_u64()) {
                items.push((p, format!("Q{}", numeric_id)));
            }
        }
    }
}
