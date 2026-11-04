// ==UserScript==
// @name         Flow2API 登录态同步
// @namespace    flow2api
// @version      1.1.0
// @description  在 Flow 页面把当前 Google Cookie 同步到 Flow2API 桌面版
// @match        https://flow.google.com/*
// @match        https://labs.google/*
// @grant        GM_cookie
// @grant        GM_xmlhttpRequest
// @connect      127.0.0.1
// @connect      localhost
// ==/UserScript==

(function () {
  "use strict";
  if (location.hostname !== "flow.google.com" && location.hostname !== "labs.google") return;

  function detectEmail() {
    for (const el of document.querySelectorAll('[aria-label]')) {
      const label = el.getAttribute('aria-label') || '';
      const match = label.match(/\(([^()\s]+@[^()\s]+\.[^()\s]+)\)/);
      if (match) return match[1].toLowerCase();
    }
    const text = document.body?.innerText || "";
    const candidates = [...text.matchAll(/[A-Z0-9._%+-]+@[A-Z0-9.-]+\.[A-Z]{2,}/gi)]
      .map(match => match[0].toLowerCase());
    return candidates[0] || "";
  }

  const panel = document.createElement("div");
  panel.style.cssText = "position:fixed;bottom:20px;right:20px;z-index:2147483647;background:#152226;color:#e7eeec;padding:14px;border:1px solid #30474b;border-radius:10px;font:13px -apple-system,BlinkMacSystemFont,'Segoe UI',sans-serif;max-width:320px;box-shadow:0 8px 24px #0008";

  const title = document.createElement("div");
  title.style.cssText = "font-weight:600;margin-bottom:8px;color:#85dfbf";
  title.textContent = "Flow2API 同步助手";

  const address = document.createElement("input");
  address.value = localStorage.getItem("flow2api_server") || "http://127.0.0.1:8000";
  address.style.cssText = "width:100%;box-sizing:border-box;background:#0d1518;color:#e7eeec;border:1px solid #314448;border-radius:6px;padding:6px 8px;font-size:12px;margin-bottom:8px";

  const button = document.createElement("button");
  button.textContent = "同步当前登录到 Flow2API";
  button.style.cssText = "width:100%;padding:8px 12px;background:#85dfbf;color:#12352b;border:0;border-radius:6px;font-weight:600;cursor:pointer";

  const status = document.createElement("div");
  status.style.cssText = "white-space:pre-wrap;overflow-wrap:anywhere;font-size:11px;line-height:1.5;color:#92a9ab;margin-top:8px";
  status.textContent = "已登录后点击上方按钮即可同步到桌面版。";

  panel.append(title, address, button, status);
  document.body.append(panel);

  button.addEventListener("click", async () => {
    const server = address.value.trim().replace(/\/+$/, "");
    if (!/^http:\/\/(localhost|127\.0\.0\.1)(:\d+)?$/.test(server)) {
      status.textContent = "请填写本机 Flow2API 地址（如 http://127.0.0.1:8000）";
      return;
    }
    localStorage.setItem("flow2api_server", server);

    const email = detectEmail();
    button.disabled = true;
    status.textContent = "正在读取当前 Cookie…";

    function listCookies(url) {
      return new Promise((resolve, reject) => {
        if (typeof GM_cookie === "undefined") {
          return reject(new Error("当前脚本管理器未开启 GM_cookie 权限，请改用 Flow2API Chrome 扩展。"));
        }
        GM_cookie.list({ url }, (cookies, error) => {
          if (error) reject(new Error(String(error)));
          else resolve(cookies || []);
        });
      });
    }

    try {
      const cookies = await listCookies(location.origin + "/");
      if (!cookies.length) {
        throw new Error("未能获取到 Cookie，请先在当前页面确认已登录 Flow。");
      }

      status.textContent = "正在发送登录态到 Flow2API…";

      GM_xmlhttpRequest({
        method: "POST",
        url: server + "/api/browser-login/sync",
        headers: { "Content-Type": "application/json" },
        data: JSON.stringify({
          login_account: email,
          google_cookies: JSON.stringify(cookies),
          protocol_mode: "protocol"
        }),
        onload: function (res) {
          button.disabled = false;
          try {
            const data = JSON.parse(res.responseText);
            if (data.success) {
              const acc = data.email || email || "Google 账号";
              const cred = (data.credits !== undefined && data.credits !== null) ? ` · 余额: ${data.credits}` : "";
              status.textContent = `✓ 同步成功！已连接 ${acc}${cred}\n请返回桌面端查看。`;
            } else {
              status.textContent = "同步失败：" + (data.detail || data.error?.message || res.responseText);
            }
          } catch (e) {
            status.textContent = "响应解析错误: " + res.responseText;
          }
        },
        onerror: function (err) {
          button.disabled = false;
          status.textContent = "连接失败，请确认 Flow2API 桌面版已启动且端口一致。";
        }
      });
    } catch (err) {
      button.disabled = false;
      status.textContent = err.message || "同步异常";
    }
  });
})();
