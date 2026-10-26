"""Durable generated media, independent from expiring delivery cache."""
import asyncio
import base64
import hashlib
import json
import os
import re
import uuid
from datetime import datetime, timezone
from pathlib import Path
from urllib.parse import urlparse


class MediaArchive:
    def __init__(self, directory=None):
        self.directory = Path(directory or Path(__file__).resolve().parents[2] / 'data' / 'generated')
        self.directory.mkdir(parents=True, exist_ok=True)

    def save_bytes(self, content, kind):
        if kind == 'image':
            if content.startswith(b'\xff\xd8\xff'): ext, mime = 'jpg', 'image/jpeg'
            elif content.startswith(b'\x89PNG\r\n\x1a\n'): ext, mime = 'png', 'image/png'
            elif content[:4] == b'RIFF' and content[8:12] == b'WEBP': ext, mime = 'webp', 'image/webp'
            elif content[:6] in (b'GIF87a', b'GIF89a'): ext, mime = 'gif', 'image/gif'
            else: raise ValueError('本机保存失败：返回内容不是支持的图片文件')
        elif kind == 'video':
            if content[4:8] == b'ftyp': ext, mime = 'mp4', 'video/mp4'
            elif content[:4] == b'\x1aE\xdf\xa3': ext, mime = 'webm', 'video/webm'
            else: raise ValueError('本机保存失败：返回内容不是支持的视频文件')
        else:
            raise ValueError('Unsupported media kind')
        digest = hashlib.sha256(content).hexdigest()
        filename = f'{digest}.{ext}'
        target = self.directory / filename
        record = {'filename': filename, 'type': kind, 'mime_type': mime,
                  'size': len(content), 'created_at': datetime.now(timezone.utc).isoformat(),
                  'url': f'/media/{filename}'}
        if not target.exists():
            self._atomic_write(target, content)
        metadata = target.with_suffix('.json')
        if not metadata.exists():
            self._atomic_write(metadata, json.dumps(record, ensure_ascii=False).encode())
        return filename

    @staticmethod
    def _atomic_write(path, content):
        temp = path.with_name('.' + uuid.uuid4().hex + '.part')
        try:
            with temp.open('xb') as f:
                f.write(content)
                f.flush()
                os.fsync(f.fileno())
            os.replace(temp, path)
        finally:
            temp.unlink(missing_ok=True)

    async def save(self, source, kind, cache):
        if source.startswith('data:'):
            content = base64.b64decode(source.split(',', 1)[1], validate=True)
        elif not source.startswith(('http://', 'https://')):
            content = base64.b64decode(source, validate=True)
        else:
            path = urlparse(source).path
            # Internal cache output is read from disk, never fetched via localhost.
            if path.startswith('/tmp/') and (cache.cache_dir / Path(path).name).is_file():
                content = await asyncio.to_thread((cache.cache_dir / Path(path).name).read_bytes)
            else:
                filename = await cache.download_and_cache(source, kind)
                content = await asyncio.to_thread(cache.get_cache_path(filename).read_bytes)
        return await asyncio.to_thread(self.save_bytes, content, kind)

    def resolve(self, filename):
        if not re.fullmatch(r'[0-9a-f]{64}\.(jpg|png|webp|gif|mp4|webm)', filename):
            return None
        path = self.directory / filename
        return path if path.is_file() else None

    def list_items(self):
        items = []
        for path in self.directory.glob('*.json'):
            try:
                record = json.loads(path.read_text())
                if self.resolve(record['filename']): items.append(record)
            except (OSError, ValueError, KeyError):
                continue
        return sorted(items, key=lambda x: x['created_at'], reverse=True)
