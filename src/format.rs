//! Shared time formatting utilities.

/// Format minutes as a human-readable duration string.
///
/// Uses floor rounding for consistency — 59.9 minutes displays as "59m", not
/// "1h 00m".
pub fn format_duration(minutes: f64) -> String {
    let hours = (minutes / 60.0) as i64;
    let mins = (minutes % 60.0) as i64;
    if hours > 0 {
        format!("{}h {:02}m", hours, mins)
    } else {
        format!("{}m", mins)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_format_duration() {
        let cases: &[(f64, &str)] = &[
            (0.0, "0m"),
            (1.0, "1m"),
            (30.0, "30m"),
            (59.0, "59m"),
            (60.0, "1h 00m"),
            (61.0, "1h 01m"),
            (90.0, "1h 30m"),
            (119.0, "1h 59m"),
            (120.0, "2h 00m"),
            (150.0, "2h 30m"),
            // Fractional minutes — floor, not round.
            (59.5, "59m"),
            (59.8, "59m"),
            (59.9, "59m"),
            (60.1, "1h 00m"),
            (60.9, "1h 00m"),
            (119.9, "1h 59m"),
            (120.1, "2h 00m"),
            // Large values.
            (240.0, "4h 00m"),
            (500.0, "8h 20m"),
        ];
        for (minutes, expected) in cases {
            assert_eq!(format_duration(*minutes), *expected, "for {minutes}");
        }
    }
}
