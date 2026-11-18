//! Compact, k9s-style ages: the one formatter every AGE column uses.

/// `42s`, `5m`, `3h`, `12d`, `2y` — the largest whole unit. Negative
/// durations (clock skew) read as `0s`.
pub fn compact(elapsed: jiff::SignedDuration) -> String {
    const MINUTE: i64 = 60;
    const HOUR: i64 = 60 * MINUTE;
    const DAY: i64 = 24 * HOUR;
    const YEAR: i64 = 365 * DAY;
    match elapsed.as_secs().max(0) {
        s if s < MINUTE => format!("{s}s"),
        s if s < HOUR => format!("{}m", s / MINUTE),
        s if s < DAY => format!("{}h", s / HOUR),
        s if s < YEAR => format!("{}d", s / DAY),
        s => format!("{}y", s / YEAR),
    }
}

/// The age of something created at `created`, or `-` when unknown.
pub fn since(created: Option<jiff::Timestamp>) -> String {
    created.map_or_else(
        || "-".to_string(),
        |t| compact(jiff::Timestamp::now().duration_since(t)),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use jiff::SignedDuration as Duration;

    #[test]
    fn ages_use_the_largest_whole_unit() {
        assert_eq!(compact(Duration::from_secs(45)), "45s");
        assert_eq!(compact(Duration::from_secs(125)), "2m");
        assert_eq!(compact(Duration::from_hours(5)), "5h");
        assert_eq!(compact(Duration::from_hours(20 * 24)), "20d");
        assert_eq!(compact(Duration::from_hours(800 * 24)), "2y");
        assert_eq!(compact(Duration::from_secs(-5)), "0s");
        assert_eq!(since(None), "-");
    }
}
