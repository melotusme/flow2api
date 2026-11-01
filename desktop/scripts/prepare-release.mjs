import {copyFileSync, existsSync, mkdirSync, readdirSync, readFileSync, writeFileSync} from 'node:fs';
import path from 'node:path';

const kind = process.argv[2];
const target = process.argv[3];
const config = JSON.parse(readFileSync('src-tauri/tauri.conf.json', 'utf8'));
const bundle = path.join('src-tauri/target', target, 'release/bundle');
const out = 'release-assets';
mkdirSync(out, {recursive: true});

function copySigned(source, asset) {
  const signaturePath = `${source}.sig`;
  if (!existsSync(signaturePath)) throw new Error(`缺少更新签名：${source}.sig`);
  copyFileSync(source, path.join(out, asset));
  copyFileSync(signaturePath, path.join(out, `${asset}.sig`));
  return readFileSync(path.join(out, `${asset}.sig`), 'utf8').trim();
}
function writePlatform(platform, asset, signature) {
  writeFileSync(path.join(out, `${platform}.json`), JSON.stringify({
    version: config.version, platform, asset, signature,
  }, null, 2));
}

if (kind === 'darwin-universal') {
  const mac = path.join(bundle, 'macos');
  const archives = readdirSync(mac).filter((name) => name.endsWith('.app.tar.gz'));
  if (archives.length !== 1) throw new Error('需要恰好一个 macOS 更新包');
  const asset = 'Flow2API.app.tar.gz';
  const signature = copySigned(path.join(mac, archives[0]), asset);
  const dmgs = readdirSync(path.join(bundle, 'dmg')).filter((name) => name.endsWith('.dmg'));
  if (dmgs.length !== 1) throw new Error('需要恰好一个 DMG');
  copyFileSync(path.join(bundle, 'dmg', dmgs[0]), path.join(out, dmgs[0]));
  for (const platform of ['darwin-aarch64', 'darwin-x86_64']) writePlatform(platform, asset, signature);
} else if (kind === 'windows-x86_64') {
  const nsis = path.join(bundle, 'nsis');
  const exes = readdirSync(nsis).filter((name) => name.endsWith('-setup.exe'));
  if (exes.length !== 1) throw new Error('需要恰好一个 NSIS 安装包');
  const asset = 'Flow2API.windows.x64.exe';
  writePlatform('windows-x86_64', asset, copySigned(path.join(nsis, exes[0]), asset));
} else {
  throw new Error('不支持的平台');
}
