//! Thin chrono re-export. We don't pull `chrono::Utc::now()` directly
//! into every repo so the timestamp format stays consistent across
//! the world DB's row columns for `created_at` /
//! `updated_at`.

use chrono::{DateTime, Utc};

/// Current time as an ISO-8601 / RFC-3339 UTC string, second
/// precision. Example: `"2026-09-18T14:32:07Z"`.
pub fn now_iso() -> String {
    Utc::now().format("%Y-%m-%dT%H:%M:%SZ").to_string()
}

/// Format a `chrono::DateTime<Utc>` as the project's wire format.
pub fn format(dt: DateTime<Utc>) -> String {
    dt.format("%Y-%m-%dT%H:%M:%SZ").to_string()
}

/// Parse a project-format timestamp back into a `DateTime<Utc>`.
pub fn parse_iso(s: &str) -> Result<DateTime<Utc>, chrono::ParseError> {
    DateTime::parse_from_rfc3339(s).map(|d| d.with_timezone(&Utc))
}

/// Format a unix timestamp (seconds since epoch) as a project-format
/// UTC string. Convenience for callers that already have an `i64`.
pub fn format_unix(secs: i64) -> String {
    let dt = chrono::DateTime::<Utc>::from_timestamp(secs, 0)
        .unwrap_or_else(|| chrono::DateTime::<Utc>::from_timestamp(0, 0).unwrap());
    dt.format("%Y-%m-%dT%H:%M:%SZ").to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn now_is_parseable() {
        let s = now_iso();
        // Must round-trip through chrono.
        let _ = parse_iso(&s).expect("now_iso() must produce a valid RFC-3339 string");
    }

    #[test]
    fn epoch_is_1970_01_01() {
        assert_eq!(format_unix(0), "1970-01-01T00:00:00Z");
    }
}
