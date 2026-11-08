"""Test-only independent CPython 3.14 timestamp sort-key corpus.

No parser implementation is shared with Rust. Emit finite JSON inputs and exact
integer UTC microseconds, including invalid/normalization-overflow fallback.
"""
from datetime import datetime, timezone
import json
import random
import math
import struct
import sys

assert sys.version_info[:2] == (3, 14), sys.version


def corpus():
    yield None
    yield False
    yield True
    yield []
    yield {"20240101": "request"}
    for value in [0, -0.0, 0.0, -1, 20240101, 20240101112,
                  20240101.12, 20240101112.0, 99991231235959,
                  1e-5, 1e20, 1e100, 10**4299, -(10**4299)]:
        yield value
    # Int/float str() can be a basic date with a digit or decimal separator.
    for year in [1, 1000, 2024, 9999]:
        for tail in ["0101", "0229", "1231"]:
            for suffix in ["", "012", "11234", "9235959", "0240000"]:
                raw = f"{year:04}{tail}{suffix}"
                yield int(raw)
                yield float(raw)
                for fraction in [0.12, 0.34, 0.5]:
                    yield float(raw) + fraction
    dates = ["0001-01-01", "9999-12-31", "2024-02-29", "20240229",
             "2020-W53", "2020W53", "2020-W53-7", "2020W537",
             "2024-W01", "2024W01", "2024-W01-1", "2024W011",
             "0000-12-31", "2024-01-00", "2024-02-30", "9999-W52-7"]
    times = ["00", "12", "23", "24", "25", "1", "123", "1234", "123456",
             "12:34", "12:34:56", "24:00", "24:00:00", "24:00:01",
             "12:60", "12:34:60", "12:3456", "1234:56", "12:", "",
             "12.5", "12,5", "12:34.5", "12:34,5", "1234567",
             "12:34:567", "12x", "12\x00", "12\x00junk"]
    for base in ["123456", "12:34:56", "24:00:00"]:
        for mark in [".", ","]:
            for frac in ["", "0", "1", "12345", "123456", "1234567",
                         "0000009", "123456\x00junk", "123456x", "１２"]:
                times.append(base + mark + frac)
    offsets = ["", "Z", "z", "+00", "-00", "+01", "-23", "+24",
               "+0130", "-01:30", "+01:02:03", "-010203,1234567",
               "+00:00:00.5", "-00:00:00.5", "+00:00:00.0000009",
               "+00:00:01.5", "+01:60", "+00:99:99", "+23:59:59.999999",
               "+24:00:00", "+01.5", "+01:02.5", "+", "+1", "+000",
               "+00\x00", "+00:00:00.123456\x00junk", "Z\x00junk"]
    for date in dates:
        yield date
        for time in times:
            for offset in offsets:
                yield date + "T" + time + offset
    for date in dates:
        for separator in ["T", "t", " ", "0", "1", ".", "+", "-", "Z",
                          "\x00", "\n", "\U0001f98a", "\u2028", "é", "１２"]:
            for time in ["12", "1234", "12:34:56.123456+01:02:03.5"]:
                yield date + separator + time
    # Exhaustive Gregorian cycle and all ISO weeks/days, with boundary years.
    for year in [0, 1, 4, 100, 400, *range(1800, 2201), 9998, 9999]:
        for month in range(0, 14):
            for day in [0, 1, 28, 29, 30, 31, 32]:
                yield f"{year:04}-{month:02}-{day:02}"
        for week in range(55):
            for day in range(9):
                yield f"{year:04}-W{week:02}-{day}"
    for prefix in ["2024W01", "2024-W01", "2024-W01-", "20240101"]:
        for width in range(10):
            for digit in "01279":
                yield prefix + digit * width
    # Offset fields are two digits, but unlike clock fields MM/SS may exceed
    # 59. Only the resulting total offset must be strictly less than 24 hours.
    for hour in range(100):
        for minute in range(100):
            for sign in ["+", "-"]:
                for form in [f"{hour:02}{minute:02}", f"{hour:02}:{minute:02}"]:
                    yield "2024-06-15T12:34:56" + sign + form
    for minute in range(100):
        for second in range(100):
            for sign in ["+", "-"]:
                yield f"2024-06-15T12:34:56{sign}00:{minute:02}:{second:02}.1234567"
    for date in ["0001-01-01", "9999-12-31"]:
        for time in ["00", "00:00:00.000001", "23:59:59.999999", "24"]:
            for offset in offsets:
                yield date + "T" + time + offset
    for year in [1000, 2024, 9999]:
        for separator in "0123456789":
            for time in ["00", "12", "23", "24", "1234", "235959", "240000"]:
                raw = f"{year}0101{separator}{time}"
                yield int(raw)
                yield float(raw)
                yield float(raw + ".12")
    # ASCII-only digits everywhere except the arbitrary single separator.
    base = "2024-02-29T12:34:56.123456+01:02:03.123456"
    for i, c in enumerate(base):
        for replacement in ["\x00", "x", " ", "１", "١", "+", "-", ":", ".", ","]:
            yield base[:i] + replacement + base[i + 1:]
    rng = random.Random(0xC0FE314)
    for _ in range(5000):
        value = struct.unpack("!d", rng.getrandbits(64).to_bytes(8))[0]
        if math.isfinite(value):
            yield value
    seeds = [d + "T" + t + z for d in dates for t in times[0:15] for z in offsets]
    for _ in range(15000):
        text = rng.choice(seeds)
        for _ in range(rng.randrange(1, 5)):
            i = rng.randrange(len(text) + 1)
            c = rng.choice("0123456789TWZ+-,.: xyz\x00\né🦊")
            text = text[:i] + c + text[i + rng.randrange(2):]
        yield text
    # Bounds are the finite profile bounds, not a small timestamp length cap.
    yield "2024-01-01T12:34:56." + "1" * (1048576 - 20)
    yield "2024W01" + "1" * (1048576 - 7)
    prefix = "2024-01-01T12:34:56+00:00:00."
    yield prefix + "0" * (1048576 - len(prefix))


def key(value):
    try:
        value = datetime.fromisoformat(str(value))
        if value.tzinfo is None:
            value = value.replace(tzinfo=timezone.utc)
        value = value.astimezone(timezone.utc)
        return ((value.toordinal() - 1) * 86400 + value.hour * 3600
                + value.minute * 60 + value.second) * 1000000 + value.microsecond
    except (ValueError, OverflowError):
        return 0


if __name__ == "__main__":
    count = accepted = 0
    for value in corpus():
        expected = key(value)
        count += 1
        accepted += expected != 0
        print(json.dumps([value, str(expected)], ensure_ascii=True, separators=(",", ":")))
    print(f"Python {sys.version.split()[0]}: {count} cases, {accepted} nonminimum keys", file=sys.stderr)
