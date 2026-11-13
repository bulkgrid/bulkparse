#!/usr/bin/env python3
"""Author controlled OCR stress cases, not photographs or real scanner captures.

Expectations and perturbations are written before rendering or recognition.
Requires macOS CoreText, NumPy, Pillow and PDFium in the OCR environment.
Never regenerate the committed fixtures during an evaluation.
"""
import hashlib
import json
from pathlib import Path
import subprocess

ROOT = Path(__file__).resolve().parents[1]


def cases():
    # All text is original project content; no borrowed document text.
    return [
        ('english', 'en', 'TimesNewRomanPSMT', [
            'Maintenance report', 'Replace valve C-731 before Thursday.',
            'Pressure: 18.75 bar. Quantity: 24.', 'Send the signed copy to the archive.']),
        ('swedish-a', 'sv', 'Arial', [
            'Årets leverans går till Västerås.', 'Öppna dörren före klockan åtta.',
            'Ärendet gäller blå och gröna lådor.', 'Pris: 1 249,50 kr. Antal: 27.']),
        ('swedish-b', 'sv', 'TimesNewRomanPSMT', [
            'På måndag hämtar Åsa nyckeln.', 'Räksmörgåsen kostar 89 kronor.',
            'Förrådet är låst efter sjutton.', 'Referens: SK-6038.']),
        ('french', 'fr', 'Arial', [
            'Compte rendu de réception', 'Élodie vérifie les pièces déjà reçues.',
            'Le coût annoncé est de 248,75 euros.', 'La façade sera réparée jeudi.']),
        ('german', 'de', 'Arial', [
            'Prüfbericht für das Außengerät', 'Öffnen Sie die Tür erst nach zwölf Uhr.',
            'Die Größe beträgt 48,5 Millimeter.', 'Auftrag: DE-5821.']),
        ('spanish', 'es', 'Arial', [
            'Informe de inspección', '¿Cuándo llegará el próximo envío?',
            'El técnico revisó dieciséis piezas.', 'Código: ES-7249. Total: 318,40 euros.']),
        ('danish', 'da', 'Arial', [
            'Årsrapport fra værkstedet', 'Søren åbner døren klokken ni.',
            'De grønne æsker står på bordet.', 'Antal: 32. Pris: 875,50 kr.']),
        ('polish', 'pl', 'Arial', [
            'Protokół odbioru części', 'Łukasz sprawdził żółtą skrzynię.',
            'Następna dostawa będzie w środę.', 'Numer: PL-3916. Liczba sztuk: 48.']),
        ('turkish', 'tr', 'Arial', [
            'Ürün kontrol raporu', 'İpek ışığı açtı ve ölçümü yaptı.',
            'Yağmur başlamadan önce kapıyı kapatın.', 'Sipariş: TR-2864. Tutar: 735,25 lira.']),
        ('chinese', 'zh', 'HiraginoSansGB-W3', [
            '仓库检查记录', '今天收到二十四个蓝色箱子。',
            '请在星期五之前完成检查。', '订单编号：CN-4826。']),
    ]


def sha(path):
    return hashlib.sha256(Path(path).read_bytes()).hexdigest()


def main():
    import numpy as np
    import pypdfium2 as pdfium
    from PIL import Image, ImageFilter
    out = ROOT/'fixtures/ocr-stress'
    out.mkdir(parents=True, exist_ok=False)
    scratch = ROOT/'target/ocr/stress-generation'
    scratch.mkdir(parents=True, exist_ok=False)
    specs = []
    plan = []
    for name, language, font, lines in cases():
        specs.append({'id':name, 'pages':[{'scan':False, 'spans':[
            {'text':text, 'x':48, 'y':730-i*36, 'size':17, 'font':font}
            for i,text in enumerate(lines)]}]})
        variants = ['clean', 'low-resolution', 'skew', 'noise'] if name in {'english','swedish-a'} else ['clean']
        for variant in variants:
            ident = f'{name}-{variant}'
            reference = out/f'{ident}.txt'
            reference.write_text('\n'.join(lines)+'\n', encoding='utf-8')
            plan.append({'id':ident,'base_id':name,'language':language,'variant':variant,
                         'source_dpi':72 if variant=='low-resolution' else 144,
                         'font':font, 'expected_lines':lines,
                         'perturbation': {'skew_degrees':3 if variant=='skew' else 0,
                             'noise_stddev':10 if variant=='noise' else 0,
                             'seed':20261110, 'jpeg_quality':95}})
    # Freeze all expectations and generation settings before observing OCR.
    (out/'plan.json').write_text(json.dumps(plan,ensure_ascii=False,indent=2)+'\n',encoding='utf-8')
    spec = out/'source-spec.json'
    spec.write_text(json.dumps(specs,ensure_ascii=False,indent=2)+'\n',encoding='utf-8')
    producer = ROOT/'target/holdout-pdfkit'
    if not producer.is_file():
        subprocess.run(['swiftc','-module-cache-path','/tmp/bulkparse-swift-cache',
                        str(ROOT/'scripts/holdout_pdfkit.swift'),'-o',str(producer)],check=True)
    subprocess.run([str(producer),'generate',str(spec),str(scratch)],check=True)
    manifest = []
    for item in plan:
        with pdfium.PdfDocument(str(scratch/(item['base_id']+'.pdf'))) as doc:
            page = doc[0]
            bitmap = page.render(scale=item['source_dpi']/72,rev_byteorder=True)
            image = bitmap.to_pil().convert('RGB')
            bitmap.close();page.close()
        if item['variant']=='skew':
            image = image.rotate(3,resample=Image.Resampling.BICUBIC,expand=False,fillcolor='white')
        elif item['variant']=='noise':
            values = np.asarray(image).astype(np.float32)*0.6+65
            rng = np.random.default_rng(20261110)
            values += rng.normal(0,10,values.shape[:2])[:,:,None]
            image = Image.fromarray(np.clip(values,0,255).astype(np.uint8)).filter(ImageFilter.GaussianBlur(0.5))
        pdf = out/(item['id']+'.pdf')
        image.save(pdf,'PDF',resolution=item['source_dpi'],quality=95,subsampling=0)
        with pdfium.PdfDocument(str(pdf)) as doc:
            page=doc[0];text=page.get_textpage()
            assert text.count_chars()==0,'unexpected hidden text layer'
            text.close();page.close()
        ref = out/(item['id']+'.txt')
        manifest.append({k:item[k] for k in ['id','base_id','language','variant']})
        manifest[-1].update({'track':'authored_stress','path':str(pdf.relative_to(ROOT)),
            'sha256':sha(pdf),'reference':str(ref.relative_to(ROOT)),'reference_sha256':sha(ref)})
    dest=ROOT/'benchmarks/ocr-profiles'
    dest.mkdir(parents=True,exist_ok=True)
    path=dest/'stress-manifest.json'
    if path.exists():raise ValueError('refusing to overwrite frozen manifest')
    path.write_text(json.dumps(manifest,indent=2)+'\n')
    print(f'Created {len(manifest)} controlled scan PDFs; not real-world accuracy evidence.')


if __name__=='__main__':main()
