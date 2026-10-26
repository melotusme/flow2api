import unittest
from src.services.generation_handler import MODEL_CONFIG
from src.services.model_metadata import describe_model


class ModelMetadataTests(unittest.TestCase):
    def test_every_listed_model_has_valid_input_contract(self):
        for name, cfg in MODEL_CONFIG.items():
            if not cfg.get('listed', True):
                continue
            with self.subTest(model=name):
                meta = describe_model(name, cfg)
                self.assertGreaterEqual(meta['max_images'], meta['min_images'])
                self.assertEqual(meta['requires_video_id'], cfg.get('video_type') == 'extend')
                if cfg['type'] == 'video':
                    self.assertEqual(meta['supports_images'], cfg.get('supports_images', False))

    def test_unknown_prices_are_not_misrepresented_as_free(self):
        for name in ['gemini-3.1-flash-image-square-4k', 'veo-3.1-fast-i2v-4s-landscape', 'veo-3.1-fast-extend-8s-landscape']:
            self.assertIsNone(describe_model(name, MODEL_CONFIG[name])['pricing']['credits'])

    def test_verified_price_and_duration_are_separate(self):
        name = 'omni-1.1-flash-4s-landscape'
        meta = describe_model(name, MODEL_CONFIG[name])
        self.assertEqual(meta['duration_seconds'], 4)
        self.assertEqual(meta['pricing']['credits'], 7)
        self.assertEqual(meta['max_images'], 3)
