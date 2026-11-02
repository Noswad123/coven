use super::*;
use crate::values::{self, Profile};
use std::io::BufRead;

fn key(text: &str) -> i128 {
    sort_key(Some(&Value::String(text.to_owned()))).unwrap()
}

#[test]
fn exact_epoch_microseconds_and_utc_normalization() {
    assert_eq!(key("0001-01-01"), 0);
    assert_eq!(key("0001-01-01T00:00:00.000001Z"), 1);
    assert_eq!(key("0001-01-02"), DAY);
    assert_eq!(key("9999-12-31T23:59:59.999999"), DAYS * DAY - 1);
    assert_eq!(key("0001-01-01T00:00:00+00:00:00.000001"), 0);
    assert_eq!(key("9999-12-31T23:59:59.999999-00:00:00.000001"), 0);
    assert_eq!(
        key("2024-01-01T12:00:00+01:02:03.1234567"),
        key("2024-01-01T10:57:56.876544")
    );
    assert_eq!(
        key("2024-01-01T12+00:00:00.5"),
        key("2024-01-01T11:59:59.5")
    );
    assert_eq!(
        key("2024-01-01T12-00:00:00.5"),
        key("2024-01-01T12:00:00.5")
    );
    assert_eq!(key("2024-01-01T12+00:00:00.0000009"), key("2024-01-01T12"));
}

#[test]
fn calendar_week_unicode_and_midnight_forms() {
    for text in [
        "2024-01-01",
        "20240101",
        "2024-W01",
        "2024W01",
        "2024-W01-1",
        "2024W011",
    ] {
        assert_eq!(key(text), 63_839_664_000_000_000, "{text}");
        for separator in ["T", " ", "\u{1f98a}", "\u{2028}", "\0"] {
            let value = format!("{text}{separator}12:34:56,123456789");
            assert_eq!(key(&value), key("2024-01-01T12:34:56.123456"), "{value:?}");
        }
    }
    assert_eq!(key("2020-W53-7"), key("2021-01-03"));
    assert_eq!(key("2024-W01-0112"), key("2024-01-01T01:12"));
    assert_eq!(key("2024W011112"), key("2024-01-01T12"));
    assert_eq!(key("2024W01112"), key("2024-01-01T12"));
    assert_eq!(key("2024-01-01T24:00:00.0000009"), key("2024-01-02"));
    assert_eq!(key("2024-01-00T24"), key("2024-01-01"));
    assert_eq!(key("0000-12-31T24"), 0);
    assert_eq!(key("0000-12-31T24-01"), 3_600_000_000);
    assert_eq!(key("2000-02-28T24"), key("2000-02-29"));
    assert_eq!(key("2000-02-29T24"), key("2000-03-01"));
    assert_eq!(key("1900-02-28T24"), key("1900-03-01"));
    for text in [
        "2021-W53",
        "2020-W54",
        "2020-W53-0",
        "2020-W53-8",
        "1900-02-29",
        "0000-01-01",
        "9999-W52-6",
        "9999-12-31T24",
        "2024-01-01T24:00:00.000001",
        "2024-01-01T12.5",
        "2024-01-01T12:34.5",
        "2024-01-01T１２:34",
        "2024-01-01TZ",
    ] {
        assert_eq!(key(text), 0, "{text}");
    }
}

#[test]
fn finite_scalar_stringification_and_c_parser_edges() {
    assert_eq!(sort_key(None).unwrap(), 0);
    for value in [
        Value::Null,
        Value::Bool(true),
        Value::Bool(false),
        Value::Array(vec![]),
        Value::object(vec![]).unwrap(),
        Value::float(-0.0).unwrap(),
        Value::integer(&"9".repeat(4300)).unwrap(),
    ] {
        assert_eq!(sort_key(Some(&value)).unwrap(), 0);
    }
    assert_eq!(
        sort_key(Some(&Value::integer("20240101112").unwrap())).unwrap(),
        key("2024-01-01T12")
    );
    assert_eq!(
        sort_key(Some(&Value::float(20240101.12).unwrap())).unwrap(),
        key("2024-01-01T12")
    );
    assert_eq!(key("2024-01-01T12x+01"), key("2024-01-01T11"));
    assert_eq!(key("2024-01-01T12\0"), key("2024-01-01T12"));
    assert_eq!(key("2024-01-01T12\0junk"), 0);
    assert_eq!(
        key("2024-01-01T12:34:56.123456\0junk"),
        key("2024-01-01T12:34:56.123456")
    );
}

#[test]
fn profile_length_fractions_and_week_digit_runs() {
    // Exercise growing inputs through the full profile bound without a flaky
    // elapsed-time assertion or an artificial short timestamp length limit.
    for length in [64, 1024, 65_536, values::MAX_STRING] {
        for (prefix, digit, expected) in [
            (
                "2024-01-01T12:34:56.",
                '1',
                key("2024-01-01T12:34:56.111111"),
            ),
            (
                "2024-01-01T12:34:56+00:00:00.",
                '0',
                key("2024-01-01T12:34:56"),
            ),
            ("2024W01", '1', key("2024-01-01T11:11:11.111111")),
        ] {
            let mut text = prefix.to_owned();
            text.extend(std::iter::repeat_n(digit, length - prefix.len()));
            assert_eq!(text.len(), length);
            assert_eq!(key(&text), expected, "prefix {prefix}, length {length}");
            text.pop();
            text.push('x');
            assert_eq!(
                key(&text),
                0,
                "invalid tail, prefix {prefix}, length {length}"
            );
        }
    }
}

#[test]
fn independent_python_314_seeded_and_systematic_exact_keys() {
    use std::{
        io::{Read, Seek, SeekFrom},
        process::{Child, Command, Stdio},
        time::{Duration, Instant},
    };

    struct OracleChild(Child);
    impl Drop for OracleChild {
        fn drop(&mut self) {
            let _ = self.0.kill();
            let _ = self.0.wait();
        }
    }

    // Files cannot fill a pipe while the parent waits, and parsing/assertions
    // begin only after the child has exited. The guard also reaps on panic.
    let mut stdout = tempfile::tempfile().unwrap();
    let mut stderr = tempfile::tempfile().unwrap();
    let mut child = OracleChild(
        Command::new("python3")
            .arg("-B")
            .arg(concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/src/checkpoint_guidance/timestamp/oracle.py"
            ))
            .stdin(Stdio::null())
            .stdout(Stdio::from(stdout.try_clone().unwrap()))
            .stderr(Stdio::from(stderr.try_clone().unwrap()))
            .spawn()
            .unwrap(),
    );
    let deadline = Instant::now() + Duration::from_secs(60);
    let status = loop {
        if let Some(status) = child.0.try_wait().unwrap() {
            break Some(status);
        }
        if Instant::now() >= deadline {
            break None;
        }
        std::thread::sleep(Duration::from_millis(25));
    };
    drop(child);
    stderr.seek(SeekFrom::Start(0)).unwrap();
    let mut diagnostics = String::new();
    stderr.read_to_string(&mut diagnostics).unwrap();
    assert!(
        status.is_some(),
        "timestamp oracle exceeded 60 seconds: {diagnostics}"
    );
    assert!(status.unwrap().success(), "{diagnostics}");
    stdout.seek(SeekFrom::Start(0)).unwrap();
    let mut reader = std::io::BufReader::new(stdout);
    let mut line = Vec::new();
    let mut count = 0;
    let mut nonminimum = 0;
    loop {
        line.clear();
        if reader.read_until(b'\n', &mut line).unwrap() == 0 {
            break;
        }
        let row = values::parse(&line, Profile::LegacySnapshot).unwrap();
        let row = row.as_array().unwrap();
        let expected = row[1].as_str().unwrap().parse::<i128>().unwrap();
        assert_eq!(
            sort_key(Some(&row[0])).unwrap(),
            expected,
            "case {count}: {}",
            String::from_utf8_lossy(&line[..line.len().min(200)])
        );
        nonminimum += usize::from(expected != 0);
        count += 1;
    }
    assert_eq!(count, 364_346);
    assert_eq!(nonminimum, 213_597);
}
