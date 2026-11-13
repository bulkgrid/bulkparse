"""Explicit CPU recognition profiles from RapidOCR 3.10.0's pinned model catalog.

This module never downloads models. Detection and orientation stay constant so
profile comparisons measure recognition changes, not a different pipeline.
"""
BASE_URL = 'https://www.modelscope.cn/models/RapidAI/RapidOCR/resolve/v3.10.0/onnx'
DETECTION = {'filename': 'PP-OCRv6_det_small.onnx',
             'sha256': '090f04abcd9d9a7498bc4ebf677e4cb9bdce1fe4197ddb7e529f1ef44e1ff94f'}
ORIENTATION = {'filename': 'ch_ppocr_mobile_v2.0_cls_mobile.onnx',
               'sha256': 'e47acedf663230f8863ff1ab0e64dd2d82b838fceb5957146dab185a89d6215c'}
PROFILES = {
    'multi-small': {
        'filename': 'PP-OCRv6_rec_small.onnx',
        'sha256': '6f327246b50388f3c176ae304bd95767ea6dc0c9ae92153ef8cbe210b3c14884',
        'params': {'Rec.ocr_version': 'PP-OCRv6', 'Rec.lang_type': 'ch', 'Rec.model_type': 'small'},
        'url': BASE_URL + '/PP-OCRv6/rec/PP-OCRv6_rec_small.onnx',
    },
    'multi-medium': {
        'filename': 'PP-OCRv6_rec_medium.onnx',
        'sha256': 'eef444829dbbe18d7fea59a3f6eb75647518d2b3a9568d27c92e42940204894b',
        'params': {'Rec.ocr_version': 'PP-OCRv6', 'Rec.lang_type': 'ch', 'Rec.model_type': 'medium'},
        'url': BASE_URL + '/PP-OCRv6/rec/PP-OCRv6_rec_medium.onnx',
    },
    'latin-mobile': {
        'filename': 'latin_PP-OCRv5_rec_mobile.onnx',
        'sha256': 'b20bd37c168a570f583afbc8cd7925603890efbcdc000a59e22c269d160b5f5a',
        'params': {'Rec.ocr_version': 'PP-OCRv5', 'Rec.lang_type': 'latin', 'Rec.model_type': 'mobile'},
        'url': BASE_URL + '/PP-OCRv5/rec/latin_PP-OCRv5_rec_mobile.onnx',
    },
}


def profile_settings(data):
    """Resolve only known, locally provisioned profiles; v1 stays compatible."""
    if data.get('version') == 1:
        if set(data) != {'version', 'models'}:
            raise ValueError('invalid version 1 model manifest')
        return {}
    if data.get('version') != 2 or set(data) != {'version', 'profile', 'models'}:
        raise ValueError('invalid model manifest')
    name = data.get('profile')
    if not isinstance(name, str) or name not in PROFILES:
        raise ValueError('unknown OCR profile')
    profile = PROFILES[name]
    for role, expected in [('det', DETECTION), ('cls', ORIENTATION), ('rec', profile)]:
        if data['models'][role].get('sha256') != expected['sha256']:
            raise ValueError(f'{role} model does not match OCR profile')
    return dict(profile['params'])
