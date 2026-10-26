(() => {

 const header=document.createElement('header');header.className='app-header';
 header.innerHTML=`<div class="app-header-inner"><a href="/test" class="brand"><span class="brand-mark" aria-hidden="true">F</span>Flow2API <span class="pill">STUDIO</span></a><nav class="app-nav" aria-label="主导航"><a href="/test">创作工作台</a><a href="/library">本机资源</a><a href="/manage#tokens" id="tabTokens">Token 管理</a><a href="/manage#settings" id="tabSettings">系统配置</a><a href="/manage#logs" id="tabLogs">请求日志</a><a href="/guide">接入文档</a></nav><span class="header-meta">Flow 媒体生成工作区</span></div>`;
 const sync=()=>{const active=location.pathname+(location.pathname==='/manage'?(location.hash||'#tokens'):'');header.querySelectorAll('nav a').forEach(a=>{if(a.getAttribute('href')===active)a.setAttribute('aria-current','page');else a.removeAttribute('aria-current')})};
 sync();window.addEventListener('hashchange',sync);window.addEventListener('flow:navigation',sync);
 document.body.prepend(header);
 window.notify=(text)=>{const t=document.createElement('div');t.className='toast';t.setAttribute('role','status');t.textContent=text;document.body.append(t);setTimeout(()=>t.remove(),2600)};
 window.copyText=async(text)=>{try{await navigator.clipboard.writeText(text);notify('已复制')}catch{notify('复制失败，请手动选择文本复制')}};
})();
