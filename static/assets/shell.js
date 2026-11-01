(() => {
  const header = document.createElement('header');
  header.className = 'app-header';
  header.innerHTML = `<div class="app-header-inner"><a href="/test" class="brand"><span class="brand-mark" aria-hidden="true">F</span>Flow2API</a><nav class="app-nav" aria-label="主导航"><a href="/test">创作工作台</a><a href="/library">本机资源</a><a href="/manage#tokens" id="tabTokens">Google 账号</a><a href="/manage#settings" id="tabSettings">系统配置</a><a href="/manage#logs" id="tabLogs">请求日志</a></nav><div class="header-tools"><a class="btn" href="/guide">接入文档</a><button type="button" class="btn" id="headerLogout">退出登录</button></div></div>`;
  const sync = () => {
    const active = location.pathname + (location.pathname === '/manage' ? (location.hash || '#tokens') : '');
    header.querySelectorAll('nav a').forEach(link => {
      if (link.getAttribute('href') === active) link.setAttribute('aria-current', 'page');
      else link.removeAttribute('aria-current');
    });
  };
  sync();
  window.addEventListener('hashchange', sync);
  window.addEventListener('flow:navigation', sync);
  document.body.prepend(header);
  document.getElementById('headerLogout').onclick = async () => {
    if (!confirm('确定要退出登录吗?')) return;
    try {
      await fetch('/api/admin/logout', {method: 'POST', credentials: 'include', headers: {'Content-Type': 'application/json'}});
    } catch { /* still leave the page */ }
    location.href = '/login';
  };
  window.notify = text => {
    const toast = document.createElement('div');
    toast.className = 'toast';
    toast.setAttribute('role', 'status');
    toast.textContent = text;
    document.body.append(toast);
    setTimeout(() => toast.remove(), 2600);
  };
  window.copyText = async text => {
    try { await navigator.clipboard.writeText(text); notify('已复制'); }
    catch { notify('复制失败，请手动选择文本复制'); }
  };
})();
