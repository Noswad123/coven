"""Independent CPython str/repr/truth oracle; test-only, Unicode 16 pinned."""
import json
import math
import random
import struct
import unicodedata

assert unicodedata.unidata_version == "16.0.0", unicodedata.unidata_version


def emit(value):
    print(json.dumps({"value": value, "text": str(value), "truthy": bool(value)}, ensure_ascii=True))


for value in [None, False, True, 0, -0, -0.0, 0.0, [], {}, "", "literal\n\x1b🦊",
              10**4299, -(10**4299), 1e-5, 1e-4, 1e15, 1e16,
              1.0000000000000002, 2.2250738585072014e-308, 5e-324,
              # Multiple shortest decimal candidates and round-to-even ties.
              1e23, 1000000000000000128.0, 2.9802322387695312e-08,
              1.2345678901234567,
              [None, False, True, 0, -0.0, {"a": ["b", {}]}]]:
    emit(value)
for text in ["'", '"', "'\"", "\\", "a'b", 'a"b', "a'\"b\\\n\t\r\b\f\x00",
             "\u00a0\u2028\u2029\u200b\U0001f98a\U000e0001\U0010ffff"]:
    emit(text)
    emit([text, {text: text}])

# Every scalar is checked in repr context, including assigned/unassigned Unicode
# 16 boundaries, all nonprintables, controls and supplementary-plane characters.
chunk = []
for code in range(0x110000):
    if 0xD800 <= code <= 0xDFFF:
        continue
    chunk.append(chr(code))
    if len(chunk) == 4096:
        emit(["".join(chunk)])
        chunk.clear()
if chunk:
    emit(["".join(chunk)])

rng = random.Random(0xC0FE)
for _ in range(1024):
    value = struct.unpack(">d", rng.getrandbits(64).to_bytes(8, "big"))[0]
    if math.isfinite(value):
        emit([value, {"float": value}])
