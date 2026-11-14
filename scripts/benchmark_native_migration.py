#!/usr/bin/env python3
"""Controlled development comparison; not a deployed-service load benchmark."""
import argparse, hashlib, json, os, platform, queue, subprocess, sys, threading, time
from pathlib import Path
import psutil
ROOT = Path(__file__).resolve().parents[1]

def docling_worker():
    import torch
    torch.set_num_threads(1)
    sys.path.append(str(ROOT/'target/ocr/venv/lib/python3.12/site-packages'))
    from docling.document_converter import DocumentConverter, ImageFormatOption, PdfFormatOption
    from docling.datamodel.base_models import InputFormat
    from docling.datamodel.pipeline_options import PdfPipelineOptions, RapidOcrOptions
    from docling.datamodel.accelerator_options import AcceleratorOptions, AcceleratorDevice
    from rapidocr import OCRVersion, LangRec, ModelType
    from rapidocr.utils.download_file import DownloadFile
    def no_download(*args, **kwargs): raise RuntimeError('runtime downloads disabled')
    DownloadFile.run = staticmethod(no_download)
    models = json.loads((ROOT/'target/ocr/profiles/multi-small/models.json').read_text())['models']
    opts = PdfPipelineOptions()
    opts.do_ocr = True; opts.do_table_structure = True
    opts.table_structure_options.do_cell_matching = True; opts.document_timeout = 120
    opts.accelerator_options = AcceleratorOptions(num_threads=1, device=AcceleratorDevice.CPU)
    opts.ocr_options = RapidOcrOptions(backend='onnxruntime', det_model_path=models['det']['path'], cls_model_path=models['cls']['path'], rec_model_path=models['rec']['path'], rapidocr_params={
      'Rec.ocr_version': OCRVersion('PP-OCRv6'), 'Rec.lang_type': LangRec('ch'), 'Rec.model_type': ModelType('small'),
      'EngineConfig.onnxruntime.intra_op_num_threads': 1, 'EngineConfig.onnxruntime.inter_op_num_threads': 1})
    engine = DocumentConverter(format_options={InputFormat.IMAGE: ImageFormatOption(pipeline_options=opts), InputFormat.PDF: PdfFormatOption(pipeline_options=opts)})
    print(json.dumps({'ready': True}), flush=True)
    for line in sys.stdin:
        request = json.loads(line); start = time.perf_counter()
        try:
            document = engine.convert(request['path']).document
            markdown = document.export_to_markdown()
            result = {'id':request['id'], 'seconds':time.perf_counter()-start, 'bytes':len(markdown.encode()), 'sha256':hashlib.sha256(markdown.encode()).hexdigest(), 'result':document.export_to_dict()}
        except Exception as e: result = {'id':request['id'],'error':str(e)}
        print(json.dumps(result), flush=True)

def main():
    ap=argparse.ArgumentParser();ap.add_argument('--docling-worker',action='store_true');ap.add_argument('--node-worker',type=Path);ap.add_argument('--out',type=Path);ap.add_argument('--engines',nargs='+',default=['native-service','docling-cpu']);ap.add_argument('--manifest',type=Path,default=ROOT/'target/scan-parity/manifest.json');a=ap.parse_args()
    if a.docling_worker:return docling_worker()
    a.out.mkdir(parents=True,exist_ok=False)
    manifest=json.loads(a.manifest.read_text())
    commands={'native-service':['node',str(a.node_worker.resolve())], 'docling-cpu':[str(ROOT/'target/competitors/docling-venv/bin/python'),str(Path(__file__).resolve()),'--docling-worker']}
    summaries=[]
    for name in a.engines:
        command=commands[name]
        folder=a.out/name;folder.mkdir()
        env={**os.environ,'OMP_NUM_THREADS':'1','OPENBLAS_NUM_THREADS':'1','RAYON_NUM_THREADS':'1','TOKENIZERS_PARALLELISM':'false','HF_HUB_OFFLINE':'1'}
        stderr=(folder/'stderr.log').open('w')
        proc=subprocess.Popen(command,stdin=subprocess.PIPE,stdout=subprocess.PIPE,stderr=stderr,text=True,bufsize=1,env=env)
        messages=queue.Queue();stop=threading.Event();peaks={'rss':0,'samples':0}; root=psutil.Process(proc.pid); children=[]
        def reader():
            try:
                for line in proc.stdout: messages.put(json.loads(line))
            except Exception as e:messages.put(e)
            messages.put(None)
        def members():
            return [root]+[psutil.Process(pid) for pid in children]
        def sample():
            while not stop.is_set():
                try:
                    peaks['rss']=max(peaks['rss'],sum(p.memory_info().rss for p in members()));peaks['samples']+=1
                except psutil.Error:pass
                stop.wait(.01)
        def cpu():
            return sum(p.cpu_times().user+p.cpu_times().system for p in members())
        threading.Thread(target=reader,daemon=True).start();sampler=threading.Thread(target=sample,daemon=True);sampler.start()
        rows=[]
        try:
            ready=messages.get(timeout=120);assert isinstance(ready,dict) and ready.get('ready'),ready
            children=ready.get('children',[])
            for run in [0,1]:
                before_cpu=cpu();start=time.perf_counter();values=[]
                for case in manifest:
                    path=ROOT/case['path'] if 'path' in case else ROOT/'target/scan-parity/inputs'/(case['id']+'.png')
                    assert hashlib.sha256(path.read_bytes()).hexdigest()==case.get('sha256',case.get('scan_sha256'))
                    proc.stdin.write(json.dumps({'id':case['id'],'path':str(path)})+'\n');proc.stdin.flush()
                    value=messages.get(timeout=150);assert isinstance(value,dict) and value.get('id')==case['id'],value
                    if run==1 and 'result' in value:(folder/(case['id']+'.json')).write_text(json.dumps(value.pop('result')))
                    else:value.pop('result',None)
                    values.append(value)
                record={'run':run,'warmup':run==0,'wall_seconds':time.perf_counter()-start,'cpu_seconds':cpu()-before_cpu,'rows':values};rows.append(record)
                print(name,'warmup' if run==0 else 'measured',round(record['wall_seconds'],3),'seconds',round(peaks['rss']/1048576),'MiB',flush=True)
        finally:
            proc.stdin.close()
            try:proc.wait(timeout=10)
            except subprocess.TimeoutExpired:proc.kill();proc.wait()
            stop.set();sampler.join();stderr.close()
        summary={'engine':name,'peak_process_tree_rss_bytes':peaks['rss'],'samples':peaks['samples'],'runs':rows}
        summaries.append(summary);(a.out/'summary.json').write_text(json.dumps(summaries,indent=2))
    (a.out/'identity.json').write_text(json.dumps({'platform':platform.platform(),'manifest':manifest,'commands':commands,'method':'Same hash-pinned development inputs from the recorded manifest. One provider at a time; full-corpus warm-up then ONE sequential measured pass. CPU thread limit one. RSS is sum of process-tree resident bytes sampled every 10 ms across warm-up/measured pass (startup sampled only after process creation), potentially double-counting shared pages. CPU is live process-tree user+system delta per pass. Native includes TypeScript/Node host + supervised Rust parser child and temporary-file IPC. Docling uses installed CPU pipeline with OCR, table structure and cell matching; recognition/detection weights match native, but orientation/layout/table models differ. Not a reconstruction of a pinned deployed image; not a stabilized median or general quality benchmark.'},indent=2))
if __name__=='__main__':main()
