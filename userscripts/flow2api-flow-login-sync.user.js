// ==UserScript==
// @name         Flow2API 登录态同步
// @namespace    flow2api
// @version      1.0.5
// @description  在 Flow 页面把当前 Google Cookie 同步到 Flow2API
// @match        https://flow.google.com/*
// @match        https://labs.google/*
// @match        http://localhost/*
// @match        http://127.0.0.1/*
// @grant        GM_setValue
// @grant        GM_getValue
// @grant        GM_deleteValue
// @grant        GM_openInTab
// @grant        GM_cookie
// @grant        GM_xmlhttpRequest
// @connect      127.0.0.1
// @connect      localhost
// ==/UserScript==

(function () {
  "use strict";
  if (["localhost", "127.0.0.1"].includes(location.hostname)) {
    const nonce = new URLSearchParams(location.hash.slice(1)).get("flow2api-sync");
    if (!nonce) return;
    const key = "flow2api-sync-" + nonce;
    const pending = GM_getValue(key);
    GM_deleteValue(key);
    if (!pending || pending.server !== location.origin || Date.now() - pending.created > 120000) return;
    const result = document.createElement("div");
    result.style.cssText = "position:fixed;top:16px;right:16px;z-index:2147483647;background:#fff;color:#111;padding:18px;border:1px solid #ccc;white-space:pre-wrap;max-width:500px";
    document.body.append(result);
    result.textContent = "正在导入 Flow 登录态…";
    fetch("/api/browser-login/sync", {method:"POST", credentials:"include", headers:{"Content-Type":"application/json"}, body:JSON.stringify(pending.payload)})
      .then(async r => { const data=await r.json(); if(!r.ok || !data.success) throw new Error(r.status===401 ? "请先登录管理后台后重新同步" : (data.errors || [data.detail || data.message]).join("\n")); return data; })
      .then(data => {result.textContent = "Flow2API 同步成功：新增 " + data.added + "，更新 " + data.updated;})
      .catch(error => {result.textContent = "Flow2API 同步失败：" + error.message;});
    return;
  }
  if (location.hostname !== "flow.google.com" && location.hostname !== "labs.google") return;
  const DEFAULT_EMAIL = "account1@example.com";
  const panel = document.createElement("div");
  panel.style.cssText = "position:fixed;bottom:20px;right:20px;z-index:2147483647;background:#fff;color:#111;padding:14px;border:1px solid #ccc;border-radius:12px;font:14px sans-serif;max-width:390px;box-shadow:0 2px 12px #0003";
  const address = document.createElement("input");
  address.value = localStorage.getItem("flow2api_server") || "http://localhost:8000";
  address.style.cssText = "width:240px;padding:6px";
  const button = document.createElement("button");
  button.textContent = "同步到 Flow2API";
  button.style.cssText = "padding:8px;margin:8px 0;cursor:pointer";
  const status = document.createElement("div");
  status.style.cssText = "white-space:pre-wrap;overflow-wrap:anywhere";
  status.textContent = DEFAULT_EMAIL;
  panel.append(address, document.createElement("br"), button, status);
  document.body.append(panel);
  button.addEventListener("click", async () => {
    const server = address.value.trim().replace(/\/+$/, "");
    if (!/^http:\/\/(localhost|127\.0\.0\.1)(:\d+)?$/.test(server)) { status.textContent = "请填写本机 Flow2API 地址"; return; }
    localStorage.setItem("flow2api_server", server);
    const email = DEFAULT_EMAIL;
    button.disabled = true;
    status.textContent = "同步中…";

  function listCookies(url) {
    return new Promise((resolve, reject) => {
      if (typeof GM_cookie === "undefined") return reject(new Error("Tampermonkey 未提供 GM_cookie"));
      GM_cookie.list({ url }, (cookies, error) => error ? reject(new Error(String(error))) : resolve(cookies || []));
    });
  }

  listCookies(location.origin + "/").then(cookies => {
    if (!cookies.some(c => c.name === "OSID" || c.name === "__Secure-OSID")) throw new Error("Tampermonkey 无法读取 Flow 必需的 HttpOnly OSID。请使用 Flow2API Chrome 扩展同步；GM_cookie 授权无法补齐此 Cookie。");
    const nonce = crypto.randomUUID();
    const key = "flow2api-sync-" + nonce;
    GM_setValue(key, {server, created:Date.now(), payload:{login_account:email,google_cookies:JSON.stringify(cookies),protocol_mode:"protocol"}});
    setTimeout(() => GM_deleteValue(key), 120000);
    GM_openInTab(server + "/manage#flow2api-sync=" + nonce, {active:true});
    status.textContent = "已打开管理后台，请查看后台同步结果";
  }).catch(error => {status.textContent = "Flow2API 同步失败：" + error.message;}).finally(() => {button.disabled=false;});
  });
})();
