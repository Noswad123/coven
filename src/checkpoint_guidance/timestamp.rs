//! Python 3.14 `datetime.fromisoformat(str(ts))` guidance ordering.
//!
//! Input is the native finite Value profile (strings <= 1 MiB). Parsing is
//! linear, with constant auxiliary space and no clock, locale or runtime calls.
//! The test-only oracle characterizes CPython's permissive separators, week
//! ambiguity, midnight rollover, and C-parser NUL/offset edge cases as well as
//! normal ISO forms. Do not replace this with the execution timestamp grammar.

use crate::values::Value;

const DAY: i128 = 86_400_000_000;
const DAYS: i128 = 3_652_059;

/// Ascending UTC microseconds since 0001-01-01; invalid/UTC overflow is minimum.
pub(super) fn sort_key(value: Option<&Value>) -> crate::Result<i128> {
    match value {
        Some(Value::String(text)) => Ok(parse(text.as_bytes()).unwrap_or(0)),
        Some(value @ Value::Number(_)) => Ok(parse(&value.compact()?).unwrap_or(0)),
        // Their Python spellings cannot be ISO datetimes; avoid repr allocation.
        _ => Ok(0),
    }
}

fn parse(text: &[u8]) -> Option<i128> {
    if text.len() < 7 {
        return None;
    }
    // CPython resolves basic week-date/digit-separator ambiguity by digit-run
    // parity, and prefers the hyphen in YYYY-Www-HH over a digit separator.
    let separator = if text.len() == 7 {
        7
    } else if text[4] == b'-' {
        if text[5] != b'W' {
            10
        } else if text.len() > 8 && text[8] == b'-' {
            if text.len() == 9 {
                return None;
            }
            if text.get(10).is_some_and(u8::is_ascii_digit) {
                8
            } else {
                10
            }
        } else {
            8
        }
    } else if text[4] == b'W' {
        let mut end = 7;
        while text.get(end).is_some_and(u8::is_ascii_digit) {
            end += 1;
        }
        if end < 9 {
            end
        } else if end % 2 == 0 {
            7
        } else {
            8
        }
    } else {
        8
    };

    let mut clock = [0; 4];
    let mut offset = 0;
    if text.len() > separator {
        // Date fields are ASCII, so this is the first byte of the arbitrary
        // single Unicode separator, never a UTF-8 continuation byte.
        let width = match text[separator] {
            0..=0x7f => 1,
            0xc0..=0xdf => 2,
            0xe0..=0xef => 3,
            0xf0..=0xf7 => 4,
            _ => return None,
        };
        let time = text.get(separator + width..)?;
        let zone = time.iter().position(|c| matches!(c, b'Z' | b'+' | b'-'));
        let end = zone.unwrap_or(time.len());
        let (fields, residue) = components(time, end)?;
        clock = fields;
        if let Some(zone) = zone {
            if time[zone] == b'Z' {
                if byte(time, zone + 1) != 0 {
                    return None;
                }
            } else {
                let tail = &time[zone + 1..];
                let (fields, residue) = components(tail, tail.len())?;
                if residue {
                    return None;
                }
                offset = ((fields[0] * 60 + fields[1]) * 60 + fields[2]) * 1_000_000 + fields[3];
                if offset >= DAY {
                    return None;
                }
                if time[zone] == b'-' {
                    offset = -offset;
                }
            }
        } else if residue {
            return None;
        }
    }
    let [hour, minute, second, micro] = clock;
    let rollover = hour == 24 && minute == 0 && second == 0 && micro == 0;
    if (!rollover && hour > 23) || minute > 59 || second > 59 {
        return None;
    }
    let day = date(text, separator, rollover)?;
    let local = day * DAY
        + if rollover { 0 } else { hour } * 3_600_000_000
        + minute * 60_000_000
        + second * 1_000_000
        + micro;
    let utc = local - offset;
    (0..DAYS * DAY).contains(&utc).then_some(utc)
}

fn byte(text: &[u8], position: usize) -> u8 {
    text.get(position).copied().unwrap_or(0)
}

fn digits(text: &[u8], position: usize, count: usize) -> Option<i128> {
    let mut value = 0;
    for digit in text.get(position..position + count)? {
        if !digit.is_ascii_digit() {
            return None;
        }
        value = value * 10 + i128::from(digit - b'0');
    }
    Some(value)
}

/// Return fields and CPython's nonterminal flag. The clock parser permits that
/// flag before a zone, but the zone parser requires it to be false. Deliberately
/// retain NUL sentinel behavior rather than imposing a stricter ISO grammar.
fn components(text: &[u8], end: usize) -> Option<([i128; 4], bool)> {
    let mut fields = [0; 4];
    let mut position = 0;
    let mut colon = false;
    for component in 0..3 {
        fields[component] = digits(text, position, 2)?;
        position += 2;
        let c = byte(text, position);
        position += 1;
        if component == 0 {
            colon = c == b':';
        }
        if matches!(c, b'.' | b',') {
            if component < 2 || position >= end {
                return None;
            }
            break;
        } else if position >= end {
            return Some((fields, c != 0));
        } else if colon && c == b':' {
            if component == 2 {
                return None;
            }
        } else if !colon {
            position -= 1;
        } else {
            return None;
        }
    }
    let count = (end.checked_sub(position)?).min(6);
    if count == 0 {
        return None;
    }
    fields[3] = digits(text, position, count)? * 10_i128.pow((6 - count) as u32);
    position += count;
    while text.get(position).is_some_and(u8::is_ascii_digit) {
        position += 1;
    }
    Some((fields, byte(text, position) != 0))
}

fn leap(year: i128) -> bool {
    year % 4 == 0 && (year % 100 != 0 || year % 400 == 0)
}

fn before_year(year: i128) -> i128 {
    let year = year - 1;
    year * 365 + year / 4 - year / 100 + year / 400
}

fn date(text: &[u8], end: usize, rollover: bool) -> Option<i128> {
    let mut year = digits(text, 0, 4)?;
    let separated = byte(text, 4) == b'-';
    let mut position = 4 + usize::from(separated);
    if byte(text, position) == b'W' {
        let week = digits(text, position + 1, 2)?;
        position += 3;
        let weekday = if position < end {
            if separated {
                if byte(text, position) != b'-' {
                    return None;
                }
                position += 1;
            }
            digits(text, position, 1)?
        } else {
            1
        };
        if !(1..=9999).contains(&year) || !(1..=7).contains(&weekday) {
            return None;
        }
        let january = before_year(year);
        let dow = january % 7;
        let weeks = if dow == 3 || (dow == 2 && leap(year)) {
            53
        } else {
            52
        };
        if !(1..=weeks).contains(&week) {
            return None;
        }
        let monday = january - dow + if dow > 3 { 7 } else { 0 };
        let day = monday + (week - 1) * 7 + weekday - 1 + i128::from(rollover);
        return (0..DAYS).contains(&day).then_some(day);
    }
    let mut month = digits(text, position, 2)?;
    position += 2;
    if separated {
        if byte(text, position) != b'-' {
            return None;
        }
        position += 1;
    }
    let mut day = digits(text, position, 2)?;
    let month_days = [
        31,
        28 + i128::from(leap(year)),
        31,
        30,
        31,
        30,
        31,
        31,
        30,
        31,
        30,
        31,
    ];
    if !(1..=12).contains(&month) {
        return None;
    }
    let limit = month_days[(month - 1) as usize];
    // Python normalizes hour 24 before validating the final calendar date.
    // Thus day 00 at midnight becomes day 01, and 0000-12-31T24 becomes year 1.
    if rollover && day <= limit {
        day += 1;
        if day > limit {
            day = 1;
            month += 1;
            if month > 12 {
                month = 1;
                year += 1;
            }
        }
    }
    if !(1..=9999).contains(&year) || day < 1 || day > limit {
        return None;
    }
    let before_month = [0, 31, 59, 90, 120, 151, 181, 212, 243, 273, 304, 334];
    Some(
        before_year(year)
            + before_month[(month - 1) as usize]
            + i128::from(month > 2 && leap(year))
            + day
            - 1,
    )
}

#[cfg(test)]
#[path = "timestamp/tests.rs"]
mod tests;
