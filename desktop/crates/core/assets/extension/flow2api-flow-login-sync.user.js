// ==UserScript==
// @name         Flow2API 登录态同步
// @namespace    flow2api
// @version      2.1.0
// @description  将当前 Flow 标签页免连接码直连到本机桌面应用，Google Cookie 始终留在浏览器
// @match        https://flow.google.com/*
// @grant        GM_xmlhttpRequest
// @grant        GM_getValue
// @grant        GM_setValue
// @grant        GM_deleteValue
// @grant        unsafeWindow
// @connect      127.0.0.1
// @connect      localhost
// @run-at       document-start
// ==/UserScript==

(function () {
  "use strict";
  if (location.hostname !== "flow.google.com" || window.top !== window) return;
  const page = typeof unsafeWindow !== "undefined" ? unsafeWindow : window;

(() => {
    if (page.__flow2apiTrustedRecaptchaHookInstalled) return;
    page.Object.defineProperty(
        page,
        '__flow2apiTrustedRecaptchaHookInstalled',
        { value: true }
    );

    const storageKey = '__flow2apiTrustedRecaptchaExecute';
    const originalBind = page.Function.prototype.bind;
    page.Function.prototype.bind = function(thisArg, ...args) {
        const bound = originalBind.call(this, thisArg, ...args);
        try {
            const enterprise = page.grecaptcha && page.grecaptcha.enterprise;
            if (
                !page[storageKey] &&
                enterprise &&
                this === enterprise.execute &&
                typeof bound === 'function'
            ) {
                page[storageKey] = bound;
            }
        } catch (error) {}
        return bound;
    };

    // New Flow replaces the public execute wrapper with an assignment that
    // overwrites the caller action by extension_hijack_detected. Preserve the
    // requested action when that exact assignment shape is used.
    page.Object.defineProperty(
        page,
        '__flow2apiRecaptchaActionGuardInstalled',
        { value: true }
    );
    const originalAssign = page.Object.assign;
    page.Object.assign = function(target, ...sources) {
        let nextSources = sources;
        if (sources.length >= 2) {
            const previous = sources[sources.length - 2];
            const last = sources[sources.length - 1];
            if (
                previous &&
                typeof previous.action === 'string' &&
                previous.action &&
                last &&
                last.action === 'extension_hijack_detected'
            ) {
                nextSources = sources.slice(0, -1);
            }
        }
        return originalAssign.call(this, target, ...nextSources);
    };
})();

  const KEY = "flow2api.connection.v2";
  // A non-secret per-tab ID prevents every Flow tab from consuming the same queue.
  let tabId = sessionStorage.getItem(KEY);
  if (!tabId) { tabId = crypto.randomUUID(); sessionStorage.setItem(KEY, tabId); }
  let connection = GM_getValue(KEY + "." + tabId, null);
  if (connection) connection.resume = true;
  let stopped = false, busy = false, timer, status;
  const delay = ms => new Promise(resolve => setTimeout(resolve, ms));
  const show = text => { if (status) status.textContent = text; };
  const save = () => GM_setValue(KEY + "." + tabId, connection);
  function email() {
    for (const el of document.querySelectorAll('[aria-label]')) {
      const label = el.getAttribute('aria-label') || '';
      if (!/Google Account|Google 账号|Google 帳戶|Google-Konto|Compte Google/i.test(label)) continue;
      const match = label.match(/[^()\s]+@[^()\s]+\.[^()\s]+/);
      if (match) return match[0].toLowerCase();
    }
    return null;
  }
  function localServer(value) {
    const u = new URL(value);
    if (u.protocol !== 'http:' || !['localhost','127.0.0.1'].includes(u.hostname) || u.username || u.password || u.search || u.hash || u.pathname !== '/') throw Error('只支持本机地址，例如 http://127.0.0.1:8000');
    return u.origin;
  }
  function request(server, path, body, token) {
    return new Promise((resolve, reject) => GM_xmlhttpRequest({
      method: 'POST', url: server + path, anonymous: true, timeout: 15000,
      headers: {'Content-Type':'application/json', ...(token ? {Authorization:'Bearer '+token} : {})},
      data: JSON.stringify(body),
      onload(r) {
        let data; try { data = JSON.parse(r.responseText); } catch { return reject(Error('本机服务返回无效响应，请确认端口属于 Flow2API')); }
        if (r.status < 200 || r.status >= 300) {
          const e = Error(data.error?.message || '连接失败（HTTP '+r.status+'）'); e.status = r.status; reject(e);
        } else resolve(data);
      },
      ontimeout: () => reject(Error('本机连接超时，请确认应用已启动')),
      onerror: () => reject(Error('无法连接本机应用；应用启动后会自动重连')),
    }));
  }
  function flowURL(value) {
    const u = new URL(value);
    if (u.origin !== 'https://flow.google.com' || u.username || u.password) throw Error('拒绝访问 Flow 以外的地址');
    return u;
  }
  async function execute(command) {
    // Every operation rechecks identity. Switching the Google account cannot cause
    // a queued request for the previous account to be submitted under the new one.
    for (let i=0; i<60 && !email(); i++) await delay(250);
    if (!email() || email() !== connection.email) throw Error('Flow 登录账号已改变，请重新连接');
    const input = command.input || {};
    switch (command.operation) {
      case 'identity': return {email: email()};
      case 'fetch': {
        const url = flowURL(input.url);
        if (!['GET','POST'].includes(input.method)) throw Error('不支持的请求方法');
        if (input.method === 'POST' && url.pathname !== '/_/AiSandboxAngularFrontend/data/batchexecute') throw Error('拒绝非 Flow RPC 请求');
        if (input.method === 'GET' && !/^\/(projects|project\/[a-f0-9-]{36})$/.test(url.pathname)) throw Error('拒绝非 Flow 页面请求');
        const response = await page.fetch(url.href, {method:input.method, credentials:'include', headers:{'Content-Type':input.content_type,'x-same-domain':'1'}, ...(input.body == null ? {} : {body:input.body}), signal:page.AbortSignal.timeout(90000)});
        return {status:response.status, text:await response.text()};
      }
      case 'navigate': {
        const url = flowURL(input.url);
        if (!/^\/(projects|project\/[a-f0-9-]{36})$/.test(url.pathname) || url.search || url.hash) throw Error('项目地址无效');
        if (location.href !== url.href) {
          connection.result = {id:command.id, value:{navigated:true}};
          save(); location.assign(url.href);
          return new Promise(()=>{}); // Completion is sent by the next document, after login is ready.
        }
        return {navigated:true};
      }
      case 'captcha': {
        if (!['IMAGE_GENERATION','VIDEO_GENERATION','UPLOAD_IMAGE'].includes(input.action)) throw Error('验证码动作无效');
        for (let i=0; i<80 && !page.grecaptcha?.enterprise; i++) await delay(250);
        const execute = page.__flow2apiTrustedRecaptchaExecute || page.grecaptcha?.enterprise?.execute;
        if (typeof execute !== 'function') throw Error('Flow 验证码尚未就绪');
        const token = await Promise.race([execute.call(page.grecaptcha.enterprise, '6LdsFiUsAAAAAIjVDZcuLhaHiDn5nnHVXVRQGeMV', {action:input.action}), delay(45000).then(()=>{throw Error('验证码超时');})]);
        if (!token) throw Error('验证码为空');
        return {token};
      }
      default: throw Error('不支持的浏览器操作');
    }
  }
  async function poll() {
    if (!connection || stopped || busy) return;
    busy = true;
    try {
      const response = await request(connection.server, '/api/browser-bridge/poll', {result:connection.result || null,resume:Boolean(connection.resume)}, connection.token);
      connection.result = null; connection.resume = false; save();
      const state = response.connection;
      show(state.state === 'ready' ? '已连接 · '+connection.email+'\n可以返回应用创作，请保持此标签页打开。' : state.message);
      for (const command of response.commands || []) {
        try { connection.result = {id:command.id, value:await execute(command)}; }
        catch (err) { connection.result = {id:command.id, error:err?.message || '浏览器操作失败，请检查 Flow 登录与网络'}; }
        save();
      }
    } catch (error) {
      show(error.message);
      if (error.status === 401) { connection=null; GM_deleteValue(KEY+'.'+tabId); }
    } finally {
      busy=false;
      if (!stopped && connection) timer=setTimeout(poll,500);
    }
  }
  let heartbeatBusy=false;
  setInterval(async()=>{
    if(!connection || stopped || heartbeatBusy)return;
    heartbeatBusy=true;
    try{await request(connection.server,'/api/browser-bridge/poll',{heartbeat:true},connection.token);}catch{}finally{heartbeatBusy=false;}
  },3000);
  function mount() {
    document.getElementById('flow2api-connection')?.remove();
    const panel = document.createElement('aside');
    panel.id = 'flow2api-connection';
    panel.style.cssText='position:fixed;bottom:20px;right:20px;z-index:2147483647;background:#152226;color:#e7eeec;padding:14px 16px;border:1px solid #30474b;border-radius:12px;font:13px system-ui;width:296px;box-shadow:0 8px 24px #0008';
    const header=document.createElement('div');
    header.style.cssText='display:flex;align-items:center;justify-content:space-between;gap:8px;margin-bottom:8px';
    const title=document.createElement('strong'); title.textContent='Flow2API · 本机直连';
    const toggle=document.createElement('button'); toggle.type='button'; toggle.textContent='收起';
    toggle.style.cssText='background:transparent;color:#9bb0b4;border:1px solid #30474b;border-radius:6px;padding:2px 8px;font:11px system-ui;cursor:pointer';
    header.append(title,toggle);
    const body=document.createElement('div');
    const address=document.createElement('input'); address.value=connection?.server || GM_getValue(KEY+'.server','http://127.0.0.1:8000'); address.placeholder='本机应用地址'; address.setAttribute('aria-label','Flow2API 本机地址');
    const code=document.createElement('input'); code.placeholder='连接码（本机直连可留空）'; code.setAttribute('aria-label','一次性连接码（可选）'); code.autocomplete='off';
    const advanced=document.createElement('details'); advanced.style.cssText='margin:8px 0';
    const summary=document.createElement('summary'); summary.textContent='端口与连接码（本机免填）'; summary.style.cssText='cursor:pointer;color:#9bb0b4;font-size:12px;user-select:none';
    for(const input of [address,code]) input.style.cssText='display:block;width:100%;box-sizing:border-box;background:#0d1518;color:#e7eeec;border:1px solid #314448;border-radius:6px;padding:8px;font:12px system-ui;margin:8px 0 0';
    advanced.append(summary,address,code);
    const button=document.createElement('button'); button.textContent='一键连接当前 Flow 账号';
    button.style.cssText='width:100%;padding:9px 10px;background:#85dfbf;color:#12352b;border:0;border-radius:6px;font-weight:600;cursor:pointer;margin-top:4px';
    status=document.createElement('div');status.setAttribute('role','status'); status.style.cssText='white-space:pre-wrap;overflow-wrap:anywhere;font-size:12px;line-height:1.55;margin-top:8px;color:#c7d6d8';
    show('本机 localhost 免填连接码：登录 Flow 后自动或一键连接，Cookie 始终保留在当前浏览器。');
    toggle.onclick=()=>{
      const hidden = body.style.display === 'none';
      body.style.display = hidden ? 'block' : 'none';
      toggle.textContent = hidden ? '收起' : '展开';
      panel.style.width = hidden ? '296px' : 'auto';
    };
    async function pairNow(silent) {
      if(busy) {if(!silent)show('浏览器正在执行操作，请等待完成后再连接');return;}
      button.disabled=true; clearTimeout(timer);
      try {
        const server=localServer(address.value.trim());
        GM_setValue(KEY+'.server', server);
        const currentEmail=email();
        if(!currentEmail) { if(!silent) throw Error('请先在当前页面登录 Google Flow'); return; }
        const paired=await request(server,'/api/browser-bridge/pair',{code:code.value.trim(),email:currentEmail});
        connection={server,token:paired.token,email:currentEmail};save();code.value='';stopped=false;
        show('已自动配对 '+currentEmail+'，正在校验身份与余额…');poll();
      } catch(error) {if(!silent)show(error.message); if(connection)poll();}
      finally {button.disabled=false;}
    }
    button.onclick=()=>pairNow(false);
    body.append(button,advanced,status);
    panel.append(header,body);document.body.append(panel);
    if(connection) {
      poll();
    } else {
      (async()=>{
        for(let i=0;i<40 && !connection;i++){
          if(email()){ await pairNow(true); break; }
          await delay(500);
        }
      })();
    }
  }
  if(document.readyState==='loading')document.addEventListener('DOMContentLoaded',mount,{once:true});else mount();
})();
