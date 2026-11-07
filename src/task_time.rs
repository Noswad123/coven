//! Task/event metadata only, never scheduling, authority, or monotonic ordering.
use crate::{Error, Result, error::require, platform};
use std::time::{SystemTime, UNIX_EPOCH};

pub(crate) fn now() -> Result<String> {
    let elapsed = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|_| Error("unsupported_task_clock"))?;
    format_utc(elapsed.as_secs(), elapsed.subsec_micros())
}

fn format_utc(seconds: u64, micros: u32) -> Result<String> {
    require(
        seconds <= 253_402_300_799 && micros < 1_000_000,
        "unsupported_task_clock",
    )?;
    // Gregorian civil date from Unix days (400-year era decomposition).
    let days = seconds / 86_400 + 719_468;
    let era = days / 146_097;
    let day_of_era = days % 146_097;
    let year_of_era =
        (day_of_era - day_of_era / 1460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let mut year = year_of_era + era * 400;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let march_month = (5 * day_of_year + 2) / 153;
    let day = day_of_year - (153 * march_month + 2) / 5 + 1;
    let month = if march_month < 10 {
        march_month + 3
    } else {
        march_month - 9
    };
    year += u64::from(month <= 2);
    let clock = seconds % 86_400;
    let fraction = if micros == 0 {
        String::new()
    } else {
        format!(".{micros:06}")
    };
    Ok(format!(
        "{year:04}-{month:02}-{day:02}T{:02}:{:02}:{:02}{fraction}+00:00",
        clock / 3600,
        clock / 60 % 60,
        clock % 60,
    ))
}

pub(crate) fn uuid_hex() -> Result<String> {
    platform::random_uuid_hex()
}

pub(crate) fn uuid_hyphenated() -> Result<String> {
    let hex = uuid_hex()?;
    Ok(format!(
        "{}-{}-{}-{}-{}",
        &hex[..8],
        &hex[8..12],
        &hex[12..16],
        &hex[16..20],
        &hex[20..]
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn utc_isoformat_matches_python_vectors_and_fraction_policy() {
        for (seconds, micros, expected) in [
            (0, 0, "1970-01-01T00:00:00+00:00"),
            (0, 1, "1970-01-01T00:00:00.000001+00:00"),
            (951_782_400, 123_456, "2000-02-29T00:00:00.123456+00:00"),
            (1_709_251_199, 999_999, "2024-02-29T23:59:59.999999+00:00"),
            (253_402_300_799, 0, "9999-12-31T23:59:59+00:00"),
        ] {
            assert_eq!(format_utc(seconds, micros).unwrap(), expected);
        }
        assert!(format_utc(253_402_300_800, 0).is_err());
        assert!(format_utc(0, 1_000_000).is_err());
    }

    #[cfg(unix)]
    #[test]
    fn command_and_event_ids_are_hyphenated_uuid4() {
        let id = uuid_hyphenated().unwrap();
        assert_eq!(id.len(), 36);
        for at in [8, 13, 18, 23] {
            assert_eq!(id.as_bytes()[at], b'-');
        }
        assert_eq!(id.as_bytes()[14], b'4');
        assert!(matches!(id.as_bytes()[19], b'8' | b'9' | b'a' | b'b'));
    }
}
