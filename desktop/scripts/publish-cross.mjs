import {createHash} from 'node:crypto';
import {existsSync, readdirSync, readFileSync, writeFileSync} from 'node:fs';
import path from 'node:path';
import zlib from 'node:zlib';
import {spawnSync} from 'node:child_process';

function buildStoredZip(entries) {
  const locals = [];
  const centrals = [];
  let offset = 0;
  for (const [name, content] of entries) {
    const nameBuf = Buffer.from(name, 'utf8');
    const crc = zlib.crc32(content) >>> 0;
    const local = Buffer.alloc(30 + nameBuf.length);
    local.writeUInt32LE(0x04034b50, 0);
    local.writeUInt16LE(20, 4);
    local.writeUInt16LE(0x0800, 6);
    local.writeUInt16LE(0, 8);
    local.writeUInt16LE(0, 10);
    local.writeUInt16LE(0x0021, 12);
    local.writeUInt32LE(crc, 14);
    local.writeUInt32LE(content.length, 18);
    local.writeUInt32LE(content.length, 22);
    local.writeUInt16LE(nameBuf.length, 26);
    local.writeUInt16LE(0, 28);
    nameBuf.copy(local, 30);
    locals.push(local, content);

    const central = Buffer.alloc(46 + nameBuf.length);
    central.writeUInt32LE(0x02014b50, 0);
    central.writeUInt16LE(20, 4);
    central.writeUInt16LE(20, 6);
    central.writeUInt16LE(0x0800, 8);
    central.writeUInt16LE(0, 10);
    central.writeUInt16LE(0, 12);
    central.writeUInt16LE(0x0021, 14);
    central.writeUInt32LE(crc, 16);
    central.writeUInt32LE(content.length, 20);
    central.writeUInt32LE(content.length, 24);
    central.writeUInt16LE(nameBuf.length, 28);
    central.writeUInt16LE(0, 30);
    central.writeUInt16LE(0, 32);
    central.writeUInt16LE(0, 34);
    central.writeUInt16LE(0, 36);
    central.writeUInt32LE(0, 38);
    central.writeUInt32LE(offset, 42);
    nameBuf.copy(central, 46);
    centrals.push(central);
    offset += local.length + content.length;
  }
  const centralBuf = Buffer.concat(centrals);
  const eocd = Buffer.alloc(22);
  eocd.writeUInt32LE(0x06054b50, 0);
  eocd.writeUInt16LE(0, 4);
  eocd.writeUInt16LE(0, 6);
  eocd.writeUInt16LE(entries.length, 8);
  eocd.writeUInt16LE(entries.length, 10);
  eocd.writeUInt32LE(centralBuf.length, 12);
  eocd.writeUInt32LE(offset, 16);
  eocd.writeUInt16LE(0, 20);
  return Buffer.concat([...locals, centralBuf, eocd]);
}

const repo = 'melotusme/flow2api-releases';
const dir = 'release-assets';
const {version} = JSON.parse(readFileSync('src-tauri/tauri.conf.json', 'utf8'));
const tag = `v${version}`;
const notes = readFileSync('release-notes.md', 'utf8');
const source_commit = process.env.SOURCE_COMMIT;
if (!/^[0-9a-f]{40}$/.test(source_commit || '')) throw new Error('发布必须记录完整源码 commit SHA');
const platforms = {};
for (const key of ['darwin-aarch64', 'darwin-x86_64', 'windows-x86_64', 'linux-x86_64']) {
  const meta = JSON.parse(readFileSync(`${dir}/${key}.json`, 'utf8'));
  if (meta.version !== version || meta.platform !== key || !meta.signature || !existsSync(`${dir}/${meta.asset}`)) {
    throw new Error(`缺少或不匹配 ${key} 的构建产物`);
  }
  if (key.startsWith('windows') && meta.asset.endsWith('.tar.gz')) throw new Error('Windows 更新地址不能指向 macOS 包');
  platforms[key] = {
    signature: meta.signature,
    url: `https://github.com/${repo}/releases/download/${tag}/${meta.asset}`,
  };
}
writeFileSync(`${dir}/latest.json`, `${JSON.stringify({version, source_commit, notes, pub_date: new Date().toISOString(), platforms}, null, 2)}\n`);
const extDir = 'crates/core/assets/chrome-extension';
if (existsSync(extDir)) {
  const extEntries = readdirSync(extDir)
    .sort()
    .map((name) => {
      if (name === 'manifest.json') {
        const manifest = JSON.parse(readFileSync(path.join(extDir, name), 'utf8'));
        manifest.version = version;
        return [name, Buffer.from(`${JSON.stringify(manifest, null, 2)}\n`, 'utf8')];
      }
      return [name, readFileSync(path.join(extDir, name))];
    });
  writeFileSync(`${dir}/Flow2API-chrome-extension.zip`, buildStoredZip(extEntries));
}
const userscriptPath = 'crates/core/assets/extension/flow2api-flow-login-sync.user.js';
if (existsSync(userscriptPath)) {
  const stamped = readFileSync(userscriptPath, 'utf8')
    .replace(/^(\s*\/\/\s*@version\s+).+$/m, `$1${version}`)
    .replace(/^(\s*const\s+VERSION\s*=\s*)["'][^"']+["'];/m, `$1"${version}";`);
  writeFileSync(`${dir}/flow2api-flow-login-sync.user.js`, stamped);
}
const files = readdirSync(dir).filter((name) => !name.endsWith('.json') || name === 'latest.json');
writeFileSync(`${dir}/SHA256SUMS`, `${files.map((name) => `${createHash('sha256').update(readFileSync(`${dir}/${name}`)).digest('hex')}  ${name}`).join('\n')}\n`);
files.push('SHA256SUMS');
const run = (args) => {
  const result = spawnSync('gh', args, {stdio: 'inherit'});
  if (result.status !== 0) throw new Error('GitHub release operation failed');
};
const existing = spawnSync('gh', ['release', 'view', tag, '--repo', repo, '--json', 'isDraft'], {encoding: 'utf8'});
if (existing.status === 0) {
  if (!JSON.parse(existing.stdout).isDraft) throw new Error('该版本已经正式发布，不能静默替换');
} else {
  if (!/release not found|HTTP 404/i.test(existing.stderr || '')) throw new Error('无法确认该版本是否存在；停止发布，请检查 GitHub 权限和网络');
  run(['release', 'create', tag, '--repo', repo, '--draft', '--title', `Flow2API ${tag}`, '--notes-file', 'release-notes.md']);
}
run(['release', 'upload', tag, '--repo', repo, '--clobber', ...files.map((name) => `${dir}/${name}`)]);
run(['release', 'edit', tag, '--repo', repo, '--draft=false', '--latest']);
