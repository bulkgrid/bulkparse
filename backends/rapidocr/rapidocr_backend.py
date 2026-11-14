#!/usr/bin/env python3
"""Supervised PDF-page OCR with optional model reuse. Local assets only."""
import argparse
import contextlib
import hashlib
import importlib.metadata
import json
import math
from pathlib import Path
import sys
from profiles import profile_settings


class BudgetError(Exception):
    pass


class AdaptiveDetector:
    """Reuse one detector session; choose resolution before recognition/cropping."""
    def __init__(self, detector):
        self.detector = detector
        self.original = detector.limit_type, detector.limit_side_len

    def __call__(self, pixels):
        detector = self.detector
        longest = max(pixels.shape[:2])
        try:
            detector.limit_type, detector.limit_side_len = 'max', 960
            result = detector(pixels)
            boxes = result.boxes
            scale = min(1.0, 960 / longest)
            # Thin text at detection scale risks disappearing or merging. Use
            # both axes for rotated text. No density gate: sparse fine print
            # needs the same protection. Empty detection also gets a retry.
            small = boxes is None or len(boxes) == 0 or any(
                min(math.dist(box[0], box[1]), math.dist(box[1], box[2])) * scale < 15
                for box in boxes)
            if longest > 960 and small:
                detector.limit_type, detector.limit_side_len = self.original
                result = detector(pixels)
            return result
        finally:
            detector.limit_type, detector.limit_side_len = self.original


def model_paths(manifest):
    if manifest.stat().st_size > 16384:
        raise BudgetError('model manifest too large')
    data = json.loads(manifest.read_text())
    if set(data.get('models', {})) != {'det', 'cls', 'rec'}:
        raise ValueError('invalid model manifest')
    params = profile_settings(data)
    result = {}
    for name, model in data['models'].items():
        path = Path(model['path'])
        if not path.is_absolute() or not path.is_file():
            raise ValueError('models must be existing absolute local files')
        digest = hashlib.sha256()
        with path.open('rb') as stream:
            for chunk in iter(lambda: stream.read(1024*1024), b''): digest.update(chunk)
        if digest.hexdigest() != model['sha256']:
            raise ValueError(f'{name} model fingerprint mismatch')
        result[name] = str(path)
    return result, params


class Runtime:
    """Verified model sessions owned by one supervised worker, never PDF results."""
    def __init__(self, models, cpu_mem_arena=False, detector_max_side=0, adaptive_detector=False):
        for name, version in [('rapidocr','3.10.0'), ('pypdfium2','5.15.2'), ('onnxruntime','1.31.1')]:
            if importlib.metadata.version(name) != version:
                raise ValueError(f'expected {name}=={version}')
        models, profile_params = model_paths(models)
        import pypdfium2 as pdfium
        import numpy as np
        import onnxruntime
        onnxruntime.disable_telemetry_events()
        from rapidocr import RapidOCR, LangRec, ModelType, OCRVersion
        # RapidOCR's programmatic API requires enum objects for version/model type;
        # the on-disk profile catalog intentionally remains plain JSON-compatible data.
        enum_types = {'Rec.ocr_version': OCRVersion, 'Rec.model_type': ModelType, 'Rec.lang_type': LangRec}
        profile_params = {key: enum_types[key](value) for key, value in profile_params.items()}
        from rapidocr.utils.download_file import DownloadFile
        def no_download(*_args, **_kwargs):
            raise ValueError('runtime downloads are disabled; provision all model assets explicitly')
        DownloadFile.run = staticmethod(no_download)
        params = {
            'Det.model_path': models['det'], 'Cls.model_path': models['cls'], 'Rec.model_path': models['rec'],
            'EngineConfig.onnxruntime.intra_op_num_threads': 1,
            'EngineConfig.onnxruntime.inter_op_num_threads': 1,
            'EngineConfig.onnxruntime.enable_cpu_mem_arena': cpu_mem_arena,
            'Global.log_level': 'error', 'Global.text_score': 0.5,
            **profile_params,
        }
        if detector_max_side:
            params.update({'Det.limit_type': 'max', 'Det.limit_side_len': detector_max_side})
        self.engine = RapidOCR(params=params)
        if adaptive_detector:
            # RapidOCR is pinned: wrap its lazy detector once, leaving classifier,
            # recognizer, preprocessing and coordinate mapping unchanged.
            self.engine.text_det = AdaptiveDetector(self.engine._load_det_model())
        self.pdfium = pdfium
        self.np = np

    def release_unused_memory(self):
        # A long-lived glibc process may retain freed inference/render buffers.
        # Trim only free heap pages; verified model sessions remain alive.
        if sys.platform.startswith('linux'):
            import ctypes
            libc = ctypes.CDLL(None)
            trim = getattr(libc, 'malloc_trim', None)
            if trim is not None:
                trim.argtypes = [ctypes.c_size_t]
                trim.restype = ctypes.c_int
                trim(0)


def pdf_pixels(args, runtime):
    pdfium, np = runtime.pdfium, runtime.np
    document = pdfium.PdfDocument(str(args.input))
    try:
        if not 1 <= args.page <= len(document): raise ValueError('page outside document')
        page = document[args.page-1]
        try:
            width, height = page.get_size()
            scale = args.dpi / 72
            if not all(math.isfinite(v) and v > 0 for v in (width, height)):
                raise ValueError('invalid page dimensions')
            if math.ceil(width*scale)*math.ceil(height*scale) > args.max_pixels:
                raise BudgetError('render pixel limit; lower DPI explicitly if appropriate')
            bitmap = page.render(scale=scale, rev_byteorder=True, limit_image_cache=True)
            try:
                if bitmap.width * bitmap.height > args.max_pixels: raise BudgetError('render pixel limit')
                image = bitmap.to_pil().convert('RGB')
                # NumPy input is BGR for RapidOCR. Only pixels enter recognition.
                pixels = np.asarray(image)[:, :, ::-1].copy()
            finally:
                bitmap.close()
        finally:
            page.close()
    finally:
        document.close()
    return pixels


def image_pixels(args, np):
    from PIL import Image, ImageOps
    import warnings
    with warnings.catch_warnings():
        warnings.simplefilter('error', Image.DecompressionBombWarning)
        with Image.open(args.input) as image:
            if image.format not in {'PNG', 'JPEG', 'GIF', 'WEBP', 'TIFF'}:
                raise ValueError('unsupported image format')
            # Inspect headers without retaining decoded frames. Bound even frame
            # counting before Pillow's TIFF/GIF n_frames can scan an unbounded chain.
            count = 0
            while True:
                try:
                    image.seek(count)
                except EOFError:
                    break
                count += 1
                if count > 2000:
                    raise BudgetError('image frame limit')
                if image.width * image.height > args.max_pixels:
                    raise BudgetError('image pixel limit')
            if not 1 <= args.page <= count:
                raise ValueError('frame outside image')
            image.seek(args.page - 1)
            frame = ImageOps.exif_transpose(image)
            if frame.mode in ('RGBA', 'LA', 'P'):
                rgba = frame.convert('RGBA')
                white = Image.new('RGBA', rgba.size, 'white')
                white.alpha_composite(rgba)
                frame = white.convert('RGB')
            else:
                frame = frame.convert('RGB')
            return np.asarray(frame)[:, :, ::-1].copy(), count


def recognize(args, runtime=None):
    runtime = runtime or Runtime(args.models, args.enable_cpu_mem_arena, args.detector_max_side, args.adaptive_detector)
    if args.input.stat().st_size > args.max_input_bytes:
        raise BudgetError('input byte limit')
    with args.input.open('rb') as stream:
        is_pdf = stream.read(5) == b'%PDF-'
    page_count = None
    if is_pdf:
        pixels = pdf_pixels(args, runtime)
    else:
        pixels, page_count = image_pixels(args, runtime.np)
    result = runtime.engine(pixels)
    texts = result.txts or ()
    if len(texts) > 4096: raise BudgetError('OCR line limit')
    if texts and (result.boxes is None or result.scores is None or
                  len(result.boxes) != len(texts) or len(result.scores) != len(texts)):
        raise ValueError('OCR engine returned inconsistent line evidence')
    lines = []
    for index, text in enumerate(texts):
        if not text.strip(): continue
        lines.append({'text': text, 'quad': result.boxes[index].tolist(),
                      'confidence': float(result.scores[index])})
    text_bytes = sum(len(line['text'].encode('utf-8')) for line in lines) + max(0, len(lines)-1)
    if text_bytes > args.max_output_bytes: raise BudgetError('OCR output byte limit')
    from tables import regions_from_grid
    regions = regions_from_grid(pixels, lines)
    result = {'image_size': [pixels.shape[1], pixels.shape[0]]}
    result.update({'regions': regions} if regions is not None else {'lines': lines})
    if page_count is not None:
        result['page_count'] = page_count
    return result


def validate(args):
    if args.adaptive_detector and args.detector_max_side:
        raise ValueError('choose either adaptive detection or a fixed detector limit')
    if args.detector_max_side != 0 and not 320 <= args.detector_max_side <= 2560:
        raise ValueError('detector maximum side must be zero or 320..2560')
    if not 72 <= args.dpi <= 600 or not 0 < args.max_pixels <= 100_000_000:
        raise ValueError('invalid rendering budgets')
    if type(args.max_output_bytes) is not int or not 0 < args.max_input_bytes <= 256 * 1024**2 or not 0 < args.max_output_bytes <= 16 * 1024**2:
        raise ValueError('invalid byte budgets')
    if not isinstance(args.page, int) or isinstance(args.page, bool) or args.page < 1:
        raise ValueError('invalid page')
    if args.input is None or args.input.stat().st_size > args.max_input_bytes:
        raise BudgetError('input byte limit')


def response(args, runtime=None):
    reply = {'version': 2, 'page': args.page}
    try:
        validate(args)
        with contextlib.redirect_stdout(sys.stderr):
            reply.update(recognize(args, runtime))
            if 'regions' in reply:
                reply['version'] = 3
    except Exception as error:
        reply['error'] = {'code': 'resource_limit' if isinstance(error, BudgetError) else 'backend_error',
                          'message': str(error)[:512]}
    return reply


def serve(args):
    runtime = None
    while True:
        line = sys.stdin.buffer.readline(16385)
        if not line:
            return
        if len(line) > 16384 or not line.endswith(b'\n'):
            raise ValueError('worker request byte limit')
        request = json.loads(line)
        if set(request) != {'version', 'id', 'input', 'page', 'max_output_bytes'} or request['version'] != 1:
            raise ValueError('invalid worker request')
        if type(request['id']) is not int or not 0 < request['id'] <= 2**64-1:
            raise ValueError('invalid worker request id')
        if not isinstance(request['input'], str) or len(request['input']) > 4096:
            raise ValueError('invalid input path')
        args.input = Path(request['input'])
        args.page = request['page']
        args.max_output_bytes = request['max_output_bytes']
        # All request budgets are checked before lazy model construction.
        try:
            validate(args)
            if runtime is None:
                with contextlib.redirect_stdout(sys.stderr):
                    runtime = Runtime(args.models, args.enable_cpu_mem_arena, args.detector_max_side, args.adaptive_detector)
            reply = response(args, runtime)
        except Exception as error:
            reply = {'version': 2, 'page': args.page,
                     'error': {'code': 'resource_limit' if isinstance(error, BudgetError) else 'backend_error',
                               'message': str(error)[:512]}}
        if args.release_unused_memory and runtime is not None:
            runtime.release_unused_memory()
        print(json.dumps({'id': request['id'], 'response': reply}, ensure_ascii=False, allow_nan=False), flush=True)
        if 'error' in reply:
            return


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--worker', action='store_true')
    parser.add_argument('--enable-cpu-mem-arena', action='store_true')
    parser.add_argument('--detector-max-side', type=int, default=0)
    parser.add_argument('--adaptive-detector', action='store_true')
    parser.add_argument('--release-unused-memory', action='store_true')
    parser.add_argument('--input', type=Path)
    parser.add_argument('--page', type=int)
    parser.add_argument('--models', type=Path, required=True)
    parser.add_argument('--dpi', type=int, required=True)
    parser.add_argument('--max-pixels', type=int, required=True)
    parser.add_argument('--max-input-bytes', type=int, required=True)
    parser.add_argument('--max-output-bytes', type=int)
    args = parser.parse_args()
    if args.worker:
        serve(args)
    else:
        if args.input is None or args.page is None or args.max_output_bytes is None:
            parser.error('single-page mode requires input, page and max-output-bytes')
        print(json.dumps(response(args), ensure_ascii=False, allow_nan=False))


if __name__ == '__main__': main()
