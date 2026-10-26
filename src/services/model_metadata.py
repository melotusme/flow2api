"""UI metadata derived from the same catalog used for generation."""
import re

PRICE_SOURCE = 'https://support.google.com/googleone?p=g1_ai_credit_menu'
PRICE_NOTE = '2026-10-19 核对当前 Flow Pro 页面；单个输出参考值，实际扣费以 Google 为准。不是美元价格，不含订阅费。'


def describe_model(model_id, cfg):
    image = cfg['type'] == 'image'
    kind = 'image' if image else cfg.get('video_type', 't2v')
    family = ('Nano Banana 2 Lite' if cfg.get('model_name') == 'HARBOR_SEAL' else
              'Nano Banana 2.1' if image else 'Omni 1.1 Flash' if kind == 'omni' else
              'Veo 3.1 ' + ('Lite' if 'lite' in model_id else 'Fast' if 'fast' in model_id else 'Quality'))
    match = re.search(r'(?:^|-)(\d+)s(?:-|$)', model_id)
    seconds = None if image else int(match[1]) if match else 8
    aspect = cfg.get('aspect_ratio', '')
    ratio = next((label for suffix, label in [('LANDSCAPE','16:9'),('PORTRAIT','9:16'),('SQUARE','1:1'),('FOUR_THREE','4:3'),('THREE_FOUR','3:4')] if aspect.endswith(suffix)), '上游默认')
    resolution = ('4K' if '4K' in str(cfg.get('upsample')) else '2K' if '2K' in str(cfg.get('upsample')) else '基础') if image else '上游默认'
    credits = None
    if image and not cfg.get('upsample'): credits = 0
    elif kind == 'omni': credits = {4:7,6:10,8:12,10:15}.get(seconds)
    elif kind == 't2v' and seconds == 8: credits = {'Veo 3.1 Lite':10,'Veo 3.1 Fast':20,'Veo 3.1 Quality':100}.get(family)
    use = {'image':'文字生成图片；上传参考图进行编辑或保持主体外观。',
           't2v':'文字生成视频，描述主体动作、镜头与场景。',
           'i2v':'首帧图生视频；两张图片按首帧、尾帧顺序输入。',
           'r2v':'参考图生成视频，用于保持角色、物体或场景外观。',
           'omni':'文字或参考图生成视频，可选 0–3 张参考图。',
           'extend':'续写已有 Flow 视频，需要源视频的 media ID。'}[kind]
    supports = True if image else bool(cfg.get('supports_images'))
    return {
        'family':family, 'kind':kind, 'type':cfg['type'], 'summary':use,
        'aspect_ratio':ratio, 'duration_seconds':seconds, 'resolution':resolution,
        'supports_images':supports, 'min_images':0 if image else cfg.get('min_images',0),
        # Images have no hard server cap; expose a conservative Studio upload limit.
        'max_images':5 if image else cfg.get('max_images',0),
        'requires_video_id':kind=='extend',
        'access_note':'模型已登记；实际权限、额度及地区限制由 Google 账号决定。',
        'pricing':{'credits':credits,'unit':'张' if image else '条','verified_at':'2026-10-19',
                   'status':'observed' if credits is not None else 'unknown','source_url':PRICE_SOURCE,'note':PRICE_NOTE,
                   'scope':'当前 Pro 账号基础图片 / 视频 720p 的页面报价；参考图、续写、放大未单独核价。'},
    }
