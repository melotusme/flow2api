'use strict';
const state = FlowList.load('library', {sort: 'created_at', order: 'desc'});
let items = [];

function matches(item) {
  const type = document.getElementById('libraryType').value;
  const q = document.getElementById('librarySearch').value.trim().toLowerCase();
  if (type !== 'all' && item.type !== type) return false;
  if (!q) return true;
  return `${item.prompt || ''} ${item.model_name || ''} ${item.model || ''}`.toLowerCase().includes(q);
}

function render() {
  const filtered = items.filter(matches).sort((a, b) => {
    const dir = state.order === 'asc' ? 1 : -1;
    if (state.sort === 'size') return (a.size - b.size) * dir;
    return String(a.created_at).localeCompare(String(b.created_at)) * dir;
  });
  document.getElementById('sortCreated').textContent = FlowList.label('时间', 'created_at', state);
  document.getElementById('sortSize').textContent = FlowList.label('大小', 'size', state);
  const {pageItems, total} = FlowList.paginate(filtered, state);
  const container = document.getElementById('items');
  const empty = document.getElementById('libraryEmpty');
  container.replaceChildren();
  empty.hidden = total > 0;
  empty.textContent = items.length ? '没有匹配的作品。换个关键词或类型再试。' : '还没有作品。生成成功后会出现在这里。';
  for (const item of pageItems) {
    const card = document.createElement('article');
    card.className = 'card';
    const media = document.createElement(item.type === 'video' ? 'video' : 'img');
    media.src = item.url;
    if (item.type === 'video') { media.controls = true; media.preload = 'metadata'; }
    else { media.alt = item.prompt || '本机保存的生成图片'; media.loading = 'lazy'; }
    const prompt = document.createElement('p');
    prompt.className = 'prompt-text';
    prompt.textContent = item.prompt || '这次生成没有记录提示词';
    const info = document.createElement('p');
    info.className = 'muted';
    const when = item.created_at ? new Date(item.created_at).toLocaleString() : '';
    info.textContent = `${item.type === 'video' ? '视频' : '图片'} · ${item.model_name || item.model || '模型未记录'} · ${(item.size / 1024 / 1024).toFixed(2)} MB · ${when}`;
    const actions = document.createElement('div');
    actions.className = 'library-actions';
    const view = document.createElement('a');
    view.className = 'btn'; view.textContent = '查看'; view.href = item.url; view.target = '_blank'; view.rel = 'noopener';
    const download = document.createElement('a');
    download.className = 'btn'; download.textContent = '下载'; download.href = `${item.url}?download=true`;
    const copy = document.createElement('button');
    copy.className = 'btn'; copy.type = 'button'; copy.textContent = '复制链接';
    copy.onclick = () => copyText(new URL(item.url, location.origin).href);
    actions.append(view, download, copy);
    card.append(media, prompt, info, actions);
    container.append(card);
  }
  FlowList.render(document.getElementById('libraryPager'), state, total, render);
}

async function loadLibrary() {
  try {
    const response = await fetch('/api/library');
    if (response.status === 401) { location.href = '/login'; return; }
    if (!response.ok) throw Error('读取本机资源失败');
    const data = await response.json();
    document.getElementById('directory').textContent = data.directory;
    document.getElementById('copyDirectory').onclick = () => copyText(data.directory);
    items = data.items || [];
    render();
  } catch (error) {
    document.getElementById('libraryEmpty').hidden = false;
    document.getElementById('libraryEmpty').textContent = error.message;
  }
}

document.getElementById('refresh').onclick = loadLibrary;
document.getElementById('librarySearch').addEventListener('input', () => { state.page = 1; render(); });
document.getElementById('libraryType').addEventListener('change', () => { state.page = 1; render(); });
document.getElementById('sortCreated').onclick = () => { FlowList.toggle(state, 'created_at'); render(); };
document.getElementById('sortSize').onclick = () => { FlowList.toggle(state, 'size'); render(); };
loadLibrary();
