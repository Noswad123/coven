//! The group contract is datetime.fromisoformat plus literal T and zero UTC
//! offset, not RFC3339. In particular basic dates, ISO weeks, reduced times,
//! comma fractions, negative zero and truncated sub-microseconds are accepted.
use crate::{Error, Result};

pub(super) fn validate(s: &str) -> Result<()> {
    if valid(s) {
        Ok(())
    } else {
        Err(Error("groups_utc_timestamp_required"))
    }
}
fn digits(s: &[u8]) -> Option<u32> {
    if s.is_empty() || !s.iter().all(u8::is_ascii_digit) {
        return None;
    }
    Some(s.iter().fold(0, |n, b| n * 10 + u32::from(b - b'0')))
}
fn leap(y: u32) -> bool {
    y % 4 == 0 && (y % 100 != 0 || y % 400 == 0)
}
fn before_year(y: u32) -> u32 {
    let y = y - 1;
    365 * y + y / 4 - y / 100 + y / 400
}
fn date(s: &[u8]) -> bool {
    let Some(y) = s
        .get(..4)
        .and_then(digits)
        .filter(|y| (1..=9999).contains(y))
    else {
        return false;
    };
    let extended = s.get(4) == Some(&b'-');
    let at = if extended { 5 } else { 4 };
    if s.get(at) == Some(&b'W') {
        let Some(w) = s.get(at + 1..at + 3).and_then(digits) else {
            return false;
        };
        let rest = &s[at + 3..];
        let day = if rest.is_empty() {
            1
        } else {
            let r = if extended {
                if rest.first() != Some(&b'-') {
                    return false;
                }
                &rest[1..]
            } else {
                rest
            };
            if r.len() != 1 {
                return false;
            }
            match digits(r) {
                Some(d) if (1..=7).contains(&d) => d,
                _ => return false,
            }
        };
        let jan1 = before_year(y) % 7; // Monday = 0, year 1 starts Monday.
        let max = if jan1 == 3 || (jan1 == 2 && leap(y)) {
            53
        } else {
            52
        };
        if w == 0 || w > max {
            return false;
        }
        let monday = i64::from(before_year(y)) + 1 - i64::from(jan1) + if jan1 > 3 { 7 } else { 0 };
        let ordinal = monday + i64::from((w - 1) * 7 + day - 1);
        ordinal >= 1 && ordinal <= i64::from(before_year(10000))
    } else {
        let (m, d) = if extended {
            if s.len() != 10 || s[7] != b'-' {
                return false;
            }
            (digits(&s[5..7]), digits(&s[8..10]))
        } else {
            if s.len() != 8 {
                return false;
            }
            (digits(&s[4..6]), digits(&s[6..8]))
        };
        let (Some(m), Some(d)) = (m, d) else {
            return false;
        };
        let days = [
            31,
            28 + u32::from(leap(y)),
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
        (1..=12).contains(&m) && d >= 1 && d <= days[(m - 1) as usize]
    }
}
// Returns hours/minutes/seconds and the first six fractional digits. Python
// truncates excess fractional digits, including those in a UTC offset.
fn time(s: &[u8]) -> Option<[u32; 4]> {
    let split = s.iter().position(|b| matches!(b, b'.' | b','));
    let (whole, fraction) = if let Some(i) = split {
        (&s[..i], Some(&s[i + 1..]))
    } else {
        (s, None)
    };
    let extended = whole.get(2) == Some(&b':');
    let mut out = [0; 4];
    let count = if extended {
        if !matches!(whole.len(), 5 | 8) || (whole.len() == 8 && whole[5] != b':') {
            return None;
        }
        out[0] = digits(&whole[..2])?;
        out[1] = digits(&whole[3..5])?;
        if whole.len() == 8 {
            out[2] = digits(&whole[6..8])?;
            3
        } else {
            2
        }
    } else {
        if !matches!(whole.len(), 2 | 4 | 6) {
            return None;
        }
        for (i, pair) in whole.chunks_exact(2).enumerate() {
            out[i] = digits(pair)?;
        }
        whole.len() / 2
    };
    if let Some(f) = fraction {
        if count != 3 || f.is_empty() || !f.iter().all(u8::is_ascii_digit) {
            return None;
        }
        for i in 0..6 {
            out[3] = out[3] * 10 + f.get(i).map_or(0, |b| u32::from(b - b'0'));
        }
    }
    Some(out)
}
fn valid(s: &str) -> bool {
    if s.is_empty() || s.len() > 64 || !s.is_ascii() {
        return false;
    }
    let Some((d, t)) = s.split_once('T') else {
        return false;
    };
    if !date(d.as_bytes()) {
        return false;
    }
    let t = t.as_bytes();
    let Some(zone) = t.iter().position(|b| matches!(b, b'Z' | b'+' | b'-')) else {
        return false;
    };
    let Some(clock) = time(&t[..zone]) else {
        return false;
    };
    if clock[1] > 59 || clock[2] > 59 {
        return false;
    }
    // CPython 3.14 accepts ISO end-of-day 24:00, normalizing to the
    // following date; truncation to microseconds precedes this check.
    if clock[0] == 24 {
        if clock[1..] != [0, 0, 0]
            || matches!(d, "9999-12-31" | "99991231" | "9999-W52-5" | "9999W525")
        {
            return false;
        }
    } else if clock[0] > 23 {
        return false;
    }
    let offset = &t[zone..];
    if offset == b"Z" {
        return true;
    }
    matches!(offset[0], b'+' | b'-') && time(&offset[1..]) == Some([0; 4])
}
