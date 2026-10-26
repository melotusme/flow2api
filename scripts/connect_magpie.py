"""Register this local Flow2API in Magpie without printing or embedding keys."""
import argparse
import json
import os
from pathlib import Path
import sqlite3
import tempfile
from datetime import datetime


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--apply', action='store_true', help='Save provider after backing up existing config')
    args = parser.parse_args()
    target = Path.home() / '.config/magpie/providers.json'
    original = target.read_bytes()
    config = json.loads(original)
    models = ['gemini-3.1-flash-lite-image-square', 'gemini-3.1-flash-image-square']
    matches = [p for p in config['providers'] if p.get('id') == 'flow2api']
    if matches and matches[0].get('chat') != 'http://127.0.0.1:8000/v1':
        raise SystemExit('Existing flow2api provider uses a different address; refusing to overwrite')
    print('Provider: Flow2API; OpenAI Chat Completions: http://127.0.0.1:8000/v1')
    print('Models: ' + ', '.join(models))
    if not args.apply:
        print('Preview only. Use --apply to save.')
        return
    root = Path(__file__).resolve().parent.parent
    with sqlite3.connect(f'file:{root / "data/flow.db"}?mode=ro', uri=True) as db:
        key = db.execute('SELECT api_key FROM admin_config LIMIT 1').fetchone()[0]
    if not key:
        raise SystemExit('Flow2API API key is empty')
    if matches:
        matches[0].update(key=key)
        matches[0]['models'] = list(dict.fromkeys(matches[0].get('models', []) + models))
    else:
        config['providers'].append({'id':'flow2api','name':'Flow2API','key':key,
                                    'chat':'http://127.0.0.1:8000/v1','models':models})
    backup_dir = target.parent / 'backups'
    backup_dir.mkdir(mode=0o700, exist_ok=True)
    backup = backup_dir / ('providers-before-flow2api-' + datetime.now().strftime('%Y%m%d-%H%M%S-%f') + '.json')
    with backup.open('xb') as out:
        os.chmod(backup, 0o600)
        out.write(original)
    if target.read_bytes() != original:
        raise SystemExit('Config changed while preparing update; retry after closing editor')
    fd, temporary = tempfile.mkstemp(prefix='.providers-', dir=target.parent)
    try:
        with os.fdopen(fd, 'w') as out:
            json.dump(config, out, ensure_ascii=False, indent=2)
            out.write('\n')
        os.replace(temporary, target)
    finally:
        if os.path.exists(temporary):
            os.unlink(temporary)
    print('Saved: ' + str(target))
    print('Backup: ' + str(backup))


if __name__ == '__main__':
    main()
