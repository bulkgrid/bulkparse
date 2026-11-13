#!/usr/bin/env python3
"""Benchmark-only persistent adapters; no production parser modifications."""
import argparse, contextlib, importlib.metadata, json, os, sys, time
from pathlib import Path

def main():
    ap=argparse.ArgumentParser(); ap.add_argument('engine'); a=ap.parse_args()
    protocol=sys.stdout
    # Libraries must not mix progress output into the JSON-lines protocol.
    sys.stdout=sys.stderr
    e=a.engine; options={}; versions={}
    if e=='liteparse':
        from liteparse import LiteParse
        options=dict(output_format='markdown',image_mode='off',ocr_enabled=False,extract_links=False,quiet=True,num_workers=1,pool_size=1)
        parser=LiteParse(**options); convert=lambda p:parser.parse(str(p)).text
        versions['liteparse']=importlib.metadata.version('liteparse')
    elif e.startswith('pymupdf4llm'):
        import pymupdf4llm
        layout=e.endswith('-layout'); pymupdf4llm.use_layout(layout)
        options=dict(layout=layout,use_ocr=False,write_images=False,embed_images=False)
        convert=lambda p:pymupdf4llm.to_markdown(str(p),use_ocr=False,write_images=False,embed_images=False)
        versions={n:importlib.metadata.version(n) for n in ['pymupdf4llm','pymupdf','pymupdf_layout']}
    elif e=='markitdown':
        from markitdown import MarkItDown
        parser=MarkItDown(enable_plugins=False); convert=lambda p:parser.convert(str(p)).text_content
        options=dict(enable_plugins=False,llm_client=None,ocr=False)
        versions['markitdown']=importlib.metadata.version('markitdown')
    elif e=='mammoth':
        import mammoth
        def convert(p):
            with p.open('rb') as f:return mammoth.convert_to_markdown(f).value
        versions['mammoth']=importlib.metadata.version('mammoth')
    elif e=='pandoc':
        import pypandoc
        formats={'docx':'docx','odt':'odt','epub':'epub','rtf':'rtf'}
        convert=lambda p:pypandoc.convert_file(str(p),'gfm',format=formats[p.suffix[1:]],extra_args=['--wrap=none'])
        options=dict(output='gfm',wrap='none',process_startup='included per document')
        versions['pandoc']=str(pypandoc.get_pandoc_version())
    elif e=='opendataloader':
        import jpype, jdk4py, opendataloader_pdf
        jar=next(Path(opendataloader_pdf.__file__).parent.rglob('*.jar'))
        jvm=next(Path(jdk4py.JAVA_HOME).rglob('libjvm.dylib'))
        jpype.startJVM(str(jvm),'-Xmx4g',classpath=[str(jar)])
        # Call CLI's exit-code-returning implementation, avoiding System.exit.
        cls=jpype.JClass('org.opendataloader.pdf.cli.CLIMain')
        strings=jpype.JArray(jpype.JString)
        method=cls.class_.getDeclaredMethod('run',strings.class_); method.setAccessible(True)
        system=jpype.JClass('java.lang.System'); system.setOut(system.err)
        options=dict(table_method='cluster',image_output='off',threads=1,hybrid=False,ocr=False,jvm='persistent JPype',heap_max='4g')
        versions={n:importlib.metadata.version(n) for n in ['opendataloader-pdf','JPype1','jdk4py']}
        def odl(p,out):
            args=strings([str(p),'--output-dir',str(out),'--format','markdown','--table-method','cluster','--image-output','off','--threads','1','--quiet'])
            code=int(method.invoke(None,jpype.JArray(jpype.JObject)([args])))
            if code:raise RuntimeError(f'OpenDataLoader CLI status {code}')
            result=out/(p.stem+'.md')
            if not result.exists():raise RuntimeError('No Markdown file returned')
            return result.read_text()
    else:raise ValueError(e)
    print(json.dumps({'ready':True,'engine':e,'options':options,'versions':versions}),file=protocol,flush=True)
    for line in sys.stdin:
        q=json.loads(line); out=Path(q['output_dir']) if q.get('output_dir') else None
        if out:out.mkdir(parents=True,exist_ok=True)
        rows=[]; batch=time.perf_counter_ns()
        for filename in q['documents']:
            p=Path(filename); started=time.perf_counter_ns(); error=None
            try: md=odl(p,out) if e=='opendataloader' else convert(p)
            except Exception as exc: md='';error=f'{type(exc).__name__}: {exc}'
            if out and (e!='opendataloader' or error): (out/(p.stem+'.md')).write_text(md)
            # Office boundary excludes protocol serialization and output writes.
            ns=time.perf_counter_ns()-started
            rows.append(dict(path=filename,document_id=p.stem,ok=error is None,error=error,status='returned' if error is None else 'error',elapsed_ns=ns,markdown=md if out is None else ''))
        elapsed=time.perf_counter_ns()-batch
        print(json.dumps(dict(elapsed_ns=elapsed,documents=rows),ensure_ascii=False),file=protocol,flush=True)

if __name__=='__main__':main()
