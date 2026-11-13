#!/usr/bin/env python3
"""Run each harness test module in isolation, including optional-artifact skips."""
from pathlib import Path
import subprocess
import sys


def main():
    root = Path(__file__).resolve().parents[1]
    failed = []
    for test in sorted((root / 'scripts').glob('test_*.py')):
        print(f'\n=== {test.name} ===', flush=True)
        result = subprocess.run(
            [sys.executable, '-m', 'unittest', 'discover', '-s', 'scripts', '-p', test.name],
            cwd=root,
        )
        if result.returncode:
            failed.append(test.name)
    if failed:
        print('Failed modules: ' + ', '.join(failed), file=sys.stderr)
    return bool(failed)


if __name__ == '__main__':
    sys.exit(main())
