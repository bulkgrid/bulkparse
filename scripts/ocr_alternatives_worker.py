#!/usr/bin/env python3
"""Benchmark-only persistent OCR worker. No production settings are changed."""
import argparse,contextlib,importlib.metadata,json,os,platform,resource,sys,time
from pathlib import Path

def usage():
 r=resource.getrusage(resource.RUSAGE_SELF)
 return {'cpu_seconds':r.ru_utime+r.ru_stime,'peak_rss_bytes':r.ru_maxrss*(1024 if sys.platform.startswith('linux') else 1)}

def main():
 ap=argparse.ArgumentParser();ap.add_argument('--engine',required=True);ap.add_argument('--size',default='small');ap.add_argument('--data',type=Path,default=Path('/data'));a=ap.parse_args()
 start=time.perf_counter();m=a.data/'models';opts={};engine=None
 with contextlib.redirect_stdout(sys.stderr):
  from PIL import Image
  if a.engine in ['rapid','lite']:
   from rapidocr import RapidOCR,ModelType,OCRVersion
   if a.engine=='rapid':
    import onnxruntime
    onnxruntime.disable_telemetry_events()
    from rapidocr.utils.download_file import DownloadFile
    def no_download(*args,**kwargs):raise RuntimeError('runtime downloads forbidden')
    DownloadFile.run=staticmethod(no_download)
   import numpy as np
   det='tiny' if a.size=='tiny' else 'small'
   rec={'small':'PP-OCRv6_rec_small.onnx','medium':'PP-OCRv6_rec_medium.onnx','tiny':'PP-OCRv6_rec_tiny.onnx','latin':'latin_PP-OCRv5_rec_mobile.onnx'}[a.size]
   opts={'Det.model_path':str(m/f'PP-OCRv6_det_{det}.onnx'),'Rec.model_path':str(m/rec),'Cls.model_path':str(m/'ch_ppocr_mobile_v2.0_cls_mobile.onnx'),'EngineConfig.onnxruntime.intra_op_num_threads':1,'EngineConfig.onnxruntime.inter_op_num_threads':1,'Global.log_level':'error','Global.text_score':0.5}
   if a.size=='latin':
    from rapidocr import LangRec
    opts.update({'Rec.ocr_version':OCRVersion.PPOCRV5,'Rec.model_type':ModelType.MOBILE,'Rec.lang_type':LangRec.LATIN})
   else:opts.update({'Det.model_type':ModelType(det),'Rec.model_type':ModelType(a.size)})
   if a.engine=='lite':opts['Rec.rec_keys_path']=str(m/('ppocrv6_tiny_dict.txt' if a.size=='tiny' else 'ppocrv6_dict.txt'))
   engine=RapidOCR(params=opts)
   if a.engine=='lite':engine._ensure_engine(need_cls=engine.use_cls)
   def recognize(p):
    pixels=np.asarray(Image.open(p).convert('RGB'))[:,:,::-1].copy()
    r=engine(pixels)
    return '\n'.join(r.txts or []),{'boxes':r.boxes.tolist() if r.boxes is not None else [],'scores':[float(s) for s in r.scores] if r.scores is not None else []}
  elif a.engine=='tesseract':
   import tesserocr
   opts={'languages':'eng+swe+fra+deu+spa+dan+pol+tur+chi_sim','psm':'AUTO','oem':'LSTM_ONLY','version':tesserocr.tesseract_version()}
   engine=tesserocr.PyTessBaseAPI(path=str(a.data/('tessdata-'+a.size)),lang=opts['languages'],psm=tesserocr.PSM.AUTO,oem=tesserocr.OEM.LSTM_ONLY)
   def recognize(p):
    engine.SetImage(Image.open(p).convert('RGB'));engine.SetSourceResolution(200)
    text=engine.GetUTF8Text();confidence=engine.MeanTextConf();engine.Clear()
    return text,{'mean_confidence':confidence}
  elif a.engine=='paddle':
   from paddleocr import PaddleOCR
   opts={'text_detection_model_name':f'PP-OCRv6_{a.size}_det','text_recognition_model_name':f'PP-OCRv6_{a.size}_rec','device':'cpu','cpu_threads':1,'enable_mkldnn':False,'use_doc_orientation_classify':False,'use_doc_unwarping':False,'use_textline_orientation':False,'text_rec_score_thresh':0.5}
   engine=PaddleOCR(**opts)
   def recognize(p):
    result=list(engine.predict(str(p)))[0]
    return '\n'.join(result['rec_texts']),{'boxes':[poly.tolist() for poly in result['rec_polys']],'scores':[float(s) for s in result['rec_scores']]}
  else:raise ValueError(a.engine)
 metadata={'platform':platform.platform(),'machine':platform.machine(),'python':sys.version,
           'packages':{d.metadata['Name']:d.version for d in importlib.metadata.distributions()}}
 for name in ['cpu.max','memory.max','pids.max']:
  p=Path('/sys/fs/cgroup')/name
  if p.exists():metadata[name]=p.read_text().strip()
 print(json.dumps({'ready':True,'init_wall_seconds':time.perf_counter()-start,'usage':usage(),'options':opts,'environment':metadata},default=str),flush=True)
 for line in sys.stdin:
  request=json.loads(line);start=time.perf_counter();before=usage();pages=[];errors=[]
  with contextlib.redirect_stdout(sys.stderr):
   for p in request['images']:
    try:
     text,detail=recognize(a.data/p);pages.append({'text':text,**detail})
    except Exception as error:pages.append({'text':''});errors.append({'path':p,'type':type(error).__name__,'message':str(error)[:800]})
  after=usage()
  print(json.dumps({'id':request['id'],'wall_seconds':time.perf_counter()-start,'cpu_seconds':after['cpu_seconds']-before['cpu_seconds'],'peak_rss_bytes':after['peak_rss_bytes'],'pages':pages,'errors':errors},ensure_ascii=False),flush=True)
if __name__=='__main__':main()
