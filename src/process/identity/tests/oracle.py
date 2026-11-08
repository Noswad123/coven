"""Test-only frozen legacy identity oracle; never launches a native probe."""
import json
from pathlib import Path
import sys

from runner.workers import _identity

print(json.dumps(_identity(Path(sys.argv[1]))))
