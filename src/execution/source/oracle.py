"""Test-only strict source oracle; never used by native source capture."""
import sys
from pathlib import Path

from runner import coven, task_attempts

value = task_attempts._source(coven.execution_source(Path(sys.argv[1]), "a" * 32), "a" * 32)
sys.stdout.buffer.write(task_attempts.canonical(value))
