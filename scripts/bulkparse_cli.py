"""Select the parse command, including archived pre-rename benchmark binaries."""
from functools import lru_cache
import re
import subprocess


@lru_cache(maxsize=None)
def parse_command(binary):
    help_text = subprocess.check_output([str(binary), '--help'], text=True, timeout=15)
    for command in ('parse', 'convert'):
        if re.search(r'^\s+' + command + r'\s', help_text, re.MULTILINE):
            return command
    raise ValueError(f'{binary}: no document parsing command in CLI help')
