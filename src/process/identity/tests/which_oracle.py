"""Test-only PATH lookup oracle; never executes the discovered program."""
import json
import shutil
import sys

print(json.dumps(shutil.which(sys.argv[1])))
