#!/usr/bin/env python3
"""Compare every rendered CSV cell with independent source CSV decoding."""
import argparse,csv,functools,hashlib,json,re
from pathlib import Path
from markdown_it import MarkdownIt

@functools.lru_cache(maxsize=65536)
def plain(value):
    tokens=MarkdownIt('commonmark').parseInline(value.strip())[0].children or []
    text=''
    for token in tokens:
        if token.type in ('text','code_inline'):text+=token.content
        elif token.type in ('softbreak','hardbreak'):text+='\n'
        elif token.type=='html_inline' and re.fullmatch(r'<br\s*/?>',token.content):text+='\n'
        elif token.type not in ('strong_open','strong_close','em_open','em_close','link_open','link_close'):
            raise ValueError(f'Unsupported inline in CSV audit: {token.type} {token.content}')
    return ' '.join(text.split())

def cells(line):
    # Pipe delimiters cannot be recognized with a one-character lookbehind:
    # a literal backslash can itself be escaped immediately before a separator.
    fields=[];start=0;escaped=False
    for i,c in enumerate(line):
        if c=='|' and not escaped:fields.append(line[start:i]);start=i+1
        escaped=not escaped if c=='\\' else False
    fields.append(line[start:])
    assert not fields[0].strip() and not fields[-1].strip(), 'not a pipe-table row'
    return [plain(v) for v in fields[1:-1]]

def main():
    ap=argparse.ArgumentParser(description=__doc__)
    ap.add_argument('--input',type=Path,required=True);ap.add_argument('--markdown',type=Path,required=True)
    ap.add_argument('--encoding',required=True);ap.add_argument('--out',type=Path,required=True)
    args=ap.parse_args();count=0;cellcount=0;issues=[]
    with args.input.open(encoding=args.encoding,newline='') as source,args.markdown.open() as output:
        expected=csv.reader(source)
        header=next(output);separator=next(output)
        assert all(not v for v in cells(header)) and '---' in separator
        for index,row in enumerate(expected,1):
            line=next(output,None)
            if line is None:issues.append(dict(row=index,error='missing output tail'));break
            actual=cells(line);want=[' '.join(v.split()) for v in row]
            if actual!=want:issues.append(dict(row=index,expected=want,actual=actual))
            count+=1;cellcount+=len(row)
        if next(output,None) is not None:issues.append(dict(error='unexpected output tail'))
    report=dict(input_sha256=hashlib.sha256(args.input.read_bytes()).hexdigest(),
        markdown_sha256=hashlib.sha256(args.markdown.read_bytes()).hexdigest(),encoding=args.encoding,
        scope='Every logical CSV record and column, in order; normalized whitespace. Encoding is an explicit assumption, not an independently certified ground truth.',
        records_checked=count,cells_checked=cellcount,issues=issues)
    args.out.write_text(json.dumps(report,indent=2)+'\n')
    print(count,'records,',cellcount,'cells,',len(issues),'differences')
    assert not issues
if __name__=='__main__':main()
