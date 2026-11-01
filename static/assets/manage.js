document.addEventListener('DOMContentLoaded', () => {
  const panel = document.getElementById('panelSettings');
  const grid = panel.querySelector('.grid');
  const cards = [...grid.children].filter(card => card.querySelector('h3'));
  const captchaCard = cards.find(card => card.querySelector('h3').textContent === '验证码');
  const captchaSelect = captchaCard?.querySelector('select');
  if (captchaSelect) {
    const help = document.createElement('div');
    help.className = 'notice';
    help.style.margin = '14px 0';
    help.textContent = '使用「已登录的 ego lite 浏览器」时，保持 Flow 标签页打开，并把控制权交给 Agent。';
    captchaSelect.parentElement.append(help);
  }

  const category = title => /验证码|代理|Chrome 扩展/.test(title)
    ? '登录与验证码'
    : /调用密钥|管理员登录/.test(title)
      ? '对外调用'
      : '生成限制';

  const controls = document.createElement('div');
  controls.className = 'settings-toolbar card';
  const lead = document.createElement('div');
  lead.className = 'selected-tools';
  const save = document.createElement('button');
  save.type = 'button';
  save.className = 'btn btn-primary';
  save.textContent = '保存更改';
  save.addEventListener('click', () => saveAllSettings());
  const description = document.createElement('p');
  description.className = 'muted';
  description.style.margin = '0';
  description.textContent = '先选择连接和验证码，再保存。修改密码和更换调用密钥仍在各自卡片里单独确认。';
  lead.append(save, description);
  controls.append(lead);

  const row = document.createElement('div');
  row.className = 'settings-filter-row';
  const search = document.createElement('input');
  search.type = 'search';
  search.placeholder = '搜索配置项';
  search.setAttribute('aria-label', '搜索系统配置');
  row.append(search);
  let selected = '全部';
  const buttons = [];
  const empty = document.createElement('p');
  empty.className = 'muted';
  empty.textContent = '没有匹配的配置。';
  empty.hidden = true;

  function apply() {
    const query = search.value.trim();
    cards.forEach(card => {
      const title = card.querySelector('h3').textContent;
      const inCategory = selected === '全部' || category(title) === selected;
      card.hidden = !inCategory || (query && !card.textContent.includes(query));
    });
    buttons.forEach(button => {
      const active = button.textContent === selected;
      button.classList.toggle('btn-primary', active);
      button.setAttribute('aria-pressed', String(active));
    });
    empty.hidden = cards.some(card => !card.hidden);
  }

  for (const name of ['全部', '登录与验证码', '对外调用', '生成限制']) {
    const button = document.createElement('button');
    button.className = 'btn';
    button.type = 'button';
    button.textContent = name;
    button.addEventListener('click', () => {
      selected = name;
      apply();
    });
    buttons.push(button);
    row.append(button);
  }
  search.addEventListener('input', apply);
  controls.append(row, empty);
  panel.prepend(controls);

  const priority = ['验证码', '代理', 'Chrome 扩展', '调用密钥', '管理员登录', '生成超时', '多个账号如何挑选', '连续失败后停用', '本机保留结果', '排错日志'];
  cards.sort((a, b) => priority.indexOf(a.querySelector('h3').textContent) - priority.indexOf(b.querySelector('h3').textContent));
  cards.forEach(card => grid.append(card));
  apply();

  const current = location.hash.slice(1);
  if (['tokens', 'settings', 'logs'].includes(current)) switchTab(current);
  window.addEventListener('hashchange', () => {
    const tab = location.hash.slice(1) || 'tokens';
    if (['tokens', 'settings', 'logs'].includes(tab)) switchTab(tab);
  });
});
