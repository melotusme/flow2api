import {createHash} from 'node:crypto';
import {existsSync, readdirSync, readFileSync, writeFileSync} from 'node:fs';
import {spawnSync} from 'node:child_process';

const repo = 'melotusme/flow2api-releases';
const dir = 'release-assets';
const {version} = JSON.parse(readFileSync('src-tauri/tauri.conf.json', 'utf8'));
const tag = `v${version}`;
const notes = readFileSync('release-notes.md', 'utf8');
const platforms = {};
for (const key of ['darwin-aarch64', 'darwin-x86_64', 'windows-x86_64']) {
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
writeFileSync(`${dir}/latest.json`, `${JSON.stringify({version, notes, pub_date: new Date().toISOString(), platforms}, null, 2)}\n`);
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
  run(['release', 'create', tag, '--repo', repo, '--draft', '--title', `Flow2API ${tag}`, '--notes-file', 'release-notes.md']);
}
run(['release', 'upload', tag, '--repo', repo, '--clobber', ...files.map((name) => `${dir}/${name}`)]);
run(['release', 'edit', tag, '--repo', repo, '--draft=false', '--latest']);
