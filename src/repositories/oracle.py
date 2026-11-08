"""Test-only legacy oracle; no Python is used by the native implementation."""
import json
import sys

from runner import repositories


def admit(raw):
    try:
        record = json.loads(raw, object_pairs_hook=repositories._pairs,
                            parse_constant=lambda _: (_ for _ in ()).throw(ValueError()))
        repositories._validate_record(record)
        return True
    except (ValueError, TypeError, repositories.RepositoryError):
        return False


request = json.load(sys.stdin)
print(json.dumps([admit(raw) for raw in request]))
