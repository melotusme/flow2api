document.addEventListener('DOMContentLoaded',()=>{
 const panel=document.getElementById('panelSettings'),grid=panel.querySelector('.grid');
 const cards=[...grid.children].filter(x=>x.querySelector('h3'));
 const captchaCard=cards.find(card=>card.querySelector('h3').textContent==='验证码配置');
 const captchaSelect=captchaCard?.querySelector('select');
 if(captchaSelect){const help=document.createElement('div');help.className='notice';help.style.margin='14px 0';help.textContent='使用「已登录的 ego lite 浏览器」时：请保持绑定的 Flow 标签页和工作区打开，并把控制权交给 Agent。切换账号或关闭工作区后，请先重新确认绑定，再尝试生成。';captchaSelect.parentElement.append(help);}
 const category=title=>/验证码|代理|插件/.test(title)?'连接与验证码':/安全|密钥/.test(title)?'账号与安全':'生成与维护';
 const controls=document.createElement('div');controls.className='settings-toolbar card';
 const description=document.createElement('p');description.className='muted';description.textContent='先配置连接方式和验证码，再按需调整生成参数。每张卡片独立保存。';controls.append(description);
 const row=document.createElement('div');row.className='settings-filter-row';const search=document.createElement('input');search.type='search';search.placeholder='搜索配置项…';search.setAttribute('aria-label','搜索系统配置');row.append(search);
 let selected='全部';const buttons=[];
 function apply(){const q=search.value.trim();cards.forEach(card=>{const title=card.querySelector('h3').textContent;card.hidden=!(selected==='全部'||category(title)===selected)||!card.textContent.includes(q)});buttons.forEach(b=>{b.classList.toggle('btn-primary',b.textContent===selected);b.setAttribute('aria-pressed',String(b.textContent===selected))})}
 for(const name of ['全部','连接与验证码','账号与安全','生成与维护']){const b=document.createElement('button');b.className='btn';b.textContent=name;b.type='button';b.onclick=()=>{selected=name;apply()};buttons.push(b);row.append(b)}
 const empty=document.createElement('p');empty.className='muted';empty.textContent='没有匹配配置，请尝试其他关键词。';empty.hidden=true;
 search.addEventListener('input',()=>{apply();empty.hidden=cards.some(c=>!c.hidden)});controls.append(row,empty);panel.prepend(controls);
 const priority=['验证码配置','代理配置','插件连接配置','API 密钥配置','生成配置','Token 轮询配置','错误处理配置','缓存配置','调试配置','安全配置'];cards.sort((a,b)=>priority.indexOf(a.querySelector('h3').textContent)-priority.indexOf(b.querySelector('h3').textContent)).forEach(card=>grid.append(card));apply();
 const current=location.hash.slice(1);if(['tokens','settings','logs'].includes(current))switchTab(current);
 window.addEventListener('hashchange',()=>{const tab=location.hash.slice(1)||'tokens';if(['tokens','settings','logs'].includes(tab))switchTab(tab)});
});
