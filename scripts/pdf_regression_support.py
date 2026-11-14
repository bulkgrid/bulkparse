"""Native PDF text accounting shared by regression evaluators."""
import json
import subprocess
from collections import Counter, defaultdict
from bulkparse_cli import parse_command

def convert(binary, source, raw=False):
    args = [str(binary), parse_command(binary), str(source), '--to', 'json']
    if raw:
        args.append('--pdf-normalize-unicode=false')
    result = subprocess.run(args, capture_output=True, timeout=45)
    return result.returncode, json.loads(result.stdout) if result.stdout else None


def page_characters(document):
    pages = defaultdict(Counter)

    def visit(value, page=None):
        if isinstance(value, dict):
            page = value.get('source', {}).get('page', page)
            if isinstance(value.get('text'), str):
                pages[page].update(c for c in value['text'] if not c.isspace())
            for key, child in value.items():
                if key not in ('source', 'text'):
                    visit(child, page)
        elif isinstance(value, list):
            for child in value:
                visit(child, page)
    visit(document['blocks'])
    return dict(pages)
