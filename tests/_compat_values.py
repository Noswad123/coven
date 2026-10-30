"""Independent CPython finite JSON/canonical/storage oracle; no native effects."""
import decimal
import json
import math
import random
import struct
import sys

sys.set_int_max_str_digits(4300)


def emit(raw, profile="authority", supported=True):
    def pairs(items):
        result = {}
        for key, value in items:
            if profile == "authority" and key in result:
                raise ValueError("duplicate")
            result[key] = value
        return result
    def constant(_):
        raise ValueError("nonfinite")
    row = {"raw": raw, "profile": profile, "valid": False}
    try:
        value = json.loads(raw, object_pairs_hook=pairs, parse_constant=constant)
        canonical = json.dumps(value, sort_keys=True, separators=(",", ":"), ensure_ascii=False, allow_nan=False)
        storage = json.dumps(value, indent=2, ensure_ascii=False, allow_nan=False)+"\n"
        canonical.encode("utf-8")
        storage.encode("utf-8")
        if supported:
            row.update(valid=True, canonical=canonical, storage=storage,
                float_bits=struct.pack(">d",value).hex() if type(value) is float else None,
                integer=str(value) if type(value) is int else None)
    except (ValueError, OverflowError, UnicodeError, RecursionError):
        pass
    print(json.dumps(row, ensure_ascii=True))


for raw in ("null", "true", "false", "0", "-0", "0.0", "-0.0", "1", "1.0", "1e0", "-0e0",
            "1e-999", "-1e-999", "1e999", "-1e999", "NaN", "Infinity", "-Infinity",
            "00", "01", "+1", ".1", "1.", "1e", "1e+", "--1", "[1,]", "{}[]", "\ufeff{}",
            '{"b":1,"a":2,"b":3}', '{"id":1,"\\u0069d":2}',
            '{"λ":[true,null,1,1.0],"nested":{"z":3,"a":-0.0}}',
            json.dumps({"\ue000":"BMP", "🦊":"astral", "é":"combining", "é":"composed",
                        "control":"\x00\b\f\n\r\t\x1f\x7f\x85\u2028\u2029\u202e", "quoted":"\"\\/"},ensure_ascii=False),
            '"\\ud800"', '"\\ud83e\\udd8a"', '"\\udfff"', '[{"a":1,"a":2}]'):
    for profile in ("legacy", "authority"):
        emit(raw, profile)

for number in (2**53-1,2**53,2**53+1,2**63-1,2**63,2**64-1,2**64,10**100,-10**100):
    emit(str(number))
for size in (4299,4300,4301):
    for sign in ("","-"):
        emit(sign+"9"*size)

# Every finite exponent region plus strategically difficult significands/signs.
for exponent in range(2047):
    for fraction in (0,1,(1<<52)-1):
        for sign in (0,1):
            bits = (sign<<63)|(exponent<<52)|fraction
            value = struct.unpack(">d",bits.to_bytes(8,"big"))[0]
            emit(repr(value))
randomizer = random.Random(20261030)
for _ in range(5000):
    bits = randomizer.getrandbits(64)
    value = struct.unpack(">d",bits.to_bytes(8,"big"))[0]
    if math.isfinite(value):
        emit(repr(value))

for center in (1e-4,1e-5,1e15,1e16,0.1,1.0):
    for value in (math.nextafter(center,-math.inf),center,math.nextafter(center,math.inf)):
        emit(repr(value))
        emit(repr(-value))

# Exact decimal midpoints and immediate decimal perturbations test parsing bits,
# not only formatter behavior on already rounded values.
decimal.getcontext().prec = 1800
for bits in (0,1,2,(1<<52)-1,1<<52,0x3fefffffffffffff,0x3ff0000000000000,
             0x3fb9999999999999,0x4340000000000000,0x7feffffffffffffe):
    lower = struct.unpack(">d",bits.to_bytes(8,"big"))[0]
    upper = struct.unpack(">d",(bits+1).to_bytes(8,"big"))[0]
    midpoint = (decimal.Decimal.from_float(lower)+decimal.Decimal.from_float(upper))/2
    delta = decimal.Decimal(1).scaleb(-1500)
    for value in (midpoint-delta,midpoint,midpoint+delta):
        raw = str(value)
        if "." not in raw and "E" not in raw:
            raw += ".0"
        emit(raw)
        emit("-"+raw)

# Explicit bounded-profile refusal, even though Python can parse these values.
emit("["*65+"0"+"]"*65, supported=False)
emit('"'+"a"*(1048576+1)+'"', supported=False)
