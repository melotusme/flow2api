import base64
import tempfile
import unittest
from pathlib import Path
from unittest.mock import AsyncMock
from src.services.media_archive import MediaArchive

PNG = base64.b64decode('iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mP8/x8AAwMCAO+a3ioAAAAASUVORK5CYII=')

class MediaArchiveTests(unittest.IsolatedAsyncioTestCase):
    async def test_download_then_persist_independent_of_cache(self):
        with tempfile.TemporaryDirectory() as d:
            cache_dir = Path(d) / 'tmp'; cache_dir.mkdir()
            cached = cache_dir / 'source'; cached.write_bytes(PNG)
            cache = type('Cache', (), {'cache_dir': cache_dir,
                'download_and_cache': AsyncMock(return_value='source'),
                'get_cache_path': lambda _, name: cache_dir / name})()
            archive = MediaArchive(Path(d) / 'saved')
            name = await archive.save('https://example.invalid/image', 'image', cache)
            cached.unlink()
            reopened = MediaArchive(archive.directory)
            self.assertEqual(reopened.resolve(name).read_bytes(), PNG)
            self.assertEqual(reopened.list_items()[0]['mime_type'], 'image/png')
            self.assertEqual(reopened.save_bytes(PNG, 'image'), name)
            self.assertEqual(len(reopened.list_items()), 1)
            self.assertIsNone(reopened.resolve('../source'))
            self.assertIsNone(reopened.resolve('abc.png'))

    async def test_base64_and_local_video_do_not_download(self):
        with tempfile.TemporaryDirectory() as d:
            cache_dir = Path(d) / 'tmp'; cache_dir.mkdir()
            video = b'\x00\x00\x00\x18ftypmp42' + b'\x00' * 30
            (cache_dir / 'v.mp4').write_bytes(video)
            cache = type('Cache', (), {'cache_dir': cache_dir, 'download_and_cache': AsyncMock()})()
            archive = MediaArchive(Path(d) / 'saved')
            await archive.save(base64.b64encode(PNG).decode(), 'image', cache)
            name = await archive.save('http://localhost:8000/tmp/v.mp4', 'video', cache)
            self.assertEqual(archive.resolve(name).read_bytes(), video)
            cache.download_and_cache.assert_not_called()
            self.assertEqual({i['type'] for i in archive.list_items()}, {'video', 'image'})

    async def test_error_page_is_not_saved_as_success(self):
        with tempfile.TemporaryDirectory() as d:
            archive = MediaArchive(d)
            with self.assertRaises(ValueError): archive.save_bytes(b'<html>403</html>', 'image')
            self.assertEqual(list(Path(d).iterdir()), [])
