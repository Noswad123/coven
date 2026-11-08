//! Persisted datetime.fromisoformat UTC acceptance, not a freshness check.
pub(super) fn valid(text: &str) -> bool {
    parse(text.as_bytes()).is_some()
}

fn byte(text: &[u8], at: usize) -> u8 {
    text.get(at).copied().unwrap_or(0)
}
fn digits(text: &[u8], at: usize, count: usize) -> Option<i64> {
    let bytes = text.get(at..at + count)?;
    bytes
        .iter()
        .all(u8::is_ascii_digit)
        .then(|| bytes.iter().fold(0, |v, c| v * 10 + i64::from(c - b'0')))
}
fn leap(year: i64) -> bool {
    year % 4 == 0 && (year % 100 != 0 || year % 400 == 0)
}
fn before_year(year: i64) -> i64 {
    let y = year - 1;
    y * 365 + y / 4 - y / 100 + y / 400
}

fn parse(text: &[u8]) -> Option<()> {
    if text.len() < 7 {
        return None;
    }
    // CPython distinguishes week-date/time ambiguities by digit-run parity.
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
    let first = *text.get(separator)?;
    let width = match first {
        0..=0x7f => 1,
        0xc0..=0xdf => 2,
        0xe0..=0xef => 3,
        0xf0..=0xf7 => 4,
        _ => return None,
    };
    let time = text.get(separator + width..)?;
    let zone = time.iter().position(|c| matches!(c, b'Z' | b'+' | b'-'))?;
    let (clock, _) = components(time, zone)?;
    if time[zone] == b'Z' {
        if byte(time, zone + 1) != 0 {
            return None;
        }
    } else {
        let tail = &time[zone + 1..];
        let (offset, residue) = components(tail, tail.len())?;
        if residue || offset != [0, 0, 0, 0] {
            return None;
        }
    }
    let [hour, minute, second, micro] = clock;
    let rollover = hour == 24 && minute == 0 && second == 0 && micro == 0;
    if (!rollover && hour > 23) || minute > 59 || second > 59 {
        return None;
    }
    date(text, separator, rollover)
}

fn components(text: &[u8], end: usize) -> Option<([i64; 4], bool)> {
    let mut fields = [0; 4];
    let mut at = 0;
    let mut colon = false;
    for component in 0..3 {
        fields[component] = digits(text, at, 2)?;
        at += 2;
        let c = byte(text, at);
        at += 1;
        if component == 0 {
            colon = c == b':';
        }
        if matches!(c, b'.' | b',') {
            if component < 2 || at >= end {
                return None;
            }
            break;
        } else if at >= end {
            return Some((fields, c != 0));
        } else if colon && c == b':' {
            if component == 2 {
                return None;
            }
        } else if !colon {
            at -= 1;
        } else {
            return None;
        }
    }
    let count = end.checked_sub(at)?.min(6);
    if count == 0 {
        return None;
    }
    fields[3] = digits(text, at, count)? * 10_i64.pow((6 - count) as u32);
    at += count;
    while text.get(at).is_some_and(u8::is_ascii_digit) {
        at += 1;
    }
    Some((fields, byte(text, at) != 0))
}

fn date(text: &[u8], end: usize, rollover: bool) -> Option<()> {
    let mut year = digits(text, 0, 4)?;
    let separated = byte(text, 4) == b'-';
    let mut at = 4 + usize::from(separated);
    if byte(text, at) == b'W' {
        let week = digits(text, at + 1, 2)?;
        at += 3;
        let weekday = if at < end {
            if separated {
                if byte(text, at) != b'-' {
                    return None;
                }
                at += 1;
            }
            digits(text, at, 1)?
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
        let day = monday + (week - 1) * 7 + weekday - 1 + i64::from(rollover);
        return (0..3_652_059).contains(&day).then_some(());
    }
    let month = digits(text, at, 2)?;
    at += 2;
    if separated {
        if byte(text, at) != b'-' {
            return None;
        }
        at += 1;
    }
    let day = digits(text, at, 2)?;
    if !(1..=12).contains(&month) {
        return None;
    }
    let limit = [
        31,
        28 + i64::from(leap(year)),
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
    ][(month - 1) as usize];
    if rollover && day == limit && month == 12 {
        year += 1;
    }
    ((1..=9999).contains(&year) && day >= i64::from(!rollover) && day <= limit).then_some(())
}
