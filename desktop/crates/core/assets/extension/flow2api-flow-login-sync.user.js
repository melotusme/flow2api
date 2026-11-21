// ==UserScript==
// @name         Flow2API 登录态同步
// @namespace    flow2api
// @version      0.4.0
// @description  将当前 Flow 与 Flow Music 标签页免连接码直连到本机桌面应用，Cookie 与令牌始终留在浏览器
// @match        https://flow.google.com/*
// @match        https://www.flowmusic.app/*
// @match        https://flowmusic.app/*
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
  if (window.top !== window) return;
  const host = location.hostname;
  const isFlow = host === "flow.google.com";
  const isMusic = host === "www.flowmusic.app" || host === "flowmusic.app";
  if (!isFlow && !isMusic) return;

  const VERSION = "0.4.0";
  const isExtensionActive = () =>
    document.documentElement?.dataset?.flow2apiBridgeActive === "1";
  if (isExtensionActive()) return;

  const page = typeof unsafeWindow !== "undefined" ? unsafeWindow : window;

  // Install reCAPTCHA hooks only on Flow, wrapped safely so sandbox/CSP quirks never abort sync.
  if (isFlow) {
    try {
      if (!page.__flow2apiTrustedRecaptchaHookInstalled) {
        page.Object.defineProperty(
          page,
          "__flow2apiTrustedRecaptchaHookInstalled",
          { value: true }
        );
        const storageKey = "__flow2apiTrustedRecaptchaExecute";
        const originalBind = page.Function.prototype.bind;
        page.Function.prototype.bind = function (thisArg, ...args) {
          const bound = originalBind.call(this, thisArg, ...args);
          try {
            const enterprise = page.grecaptcha && page.grecaptcha.enterprise;
            if (
              !page[storageKey] &&
              enterprise &&
              this === enterprise.execute &&
              typeof bound === "function"
            ) {
              page[storageKey] = bound;
            }
          } catch {}
          return bound;
        };
        page.Object.defineProperty(
          page,
          "__flow2apiRecaptchaActionGuardInstalled",
          { value: true }
        );
        const originalAssign = page.Object.assign;
        page.Object.assign = function (target, ...sources) {
          let nextSources = sources;
          if (sources.length >= 2) {
            const previous = sources[sources.length - 2];
            const last = sources[sources.length - 1];
            if (
              previous &&
              typeof previous.action === "string" &&
              previous.action &&
              last &&
              last.action === "extension_hijack_detected"
            ) {
              nextSources = sources.slice(0, -1);
            }
          }
          return originalAssign.call(this, target, ...nextSources);
        };
      }
    } catch {}
  }

  const SERVICE = isMusic ? "music" : "flow";
  const PAIR_PATH = isMusic ? "/api/music-bridge/pair" : "/api/browser-bridge/pair";
  const POLL_PATH = isMusic ? "/api/music-bridge/poll" : "/api/browser-bridge/poll";
  const LABEL = isMusic ? "Flow Music" : "Flow";
  const KEY = "flow2api.connection.v2." + SERVICE;
  const SERVER_KEY = "flow2api.connection.v2.server";

  let tabId = null;
  try {
    tabId = sessionStorage.getItem(KEY);
    if (!tabId) {
      tabId = crypto.randomUUID();
      sessionStorage.setItem(KEY, tabId);
    }
  } catch {
    tabId = crypto.randomUUID();
  }

  let connection = GM_getValue(KEY + "." + tabId, null);
  if (connection) connection.resume = true;

  let stopped = false;
  let busy = false;
  let pairing = false;
  let timer = null;
  let statusEl = null;
  let dotEl = null;
  let titleEl = null;
  let addressInput = null;
  let connectBtn = null;

  const delay = (ms) => new Promise((resolve) => setTimeout(resolve, ms));
  const save = () => GM_setValue(KEY + "." + tabId, connection);
  const clearConnection = () => {
    connection = null;
    GM_deleteValue(KEY + "." + tabId);
  };

  function yieldToExtensionIfActive() {
    if (!isExtensionActive()) return false;
    stopped = true;
    clearTimeout(timer);
    return true;
  }

  function setStatus(text, tone) {
    if (statusEl) statusEl.textContent = text;
    if (dotEl) {
      dotEl.style.background =
        tone === "ok"
          ? "#85dfbf"
          : tone === "warn"
            ? "#e5d09b"
            : tone === "error"
              ? "#ed8f86"
              : "#9c9fa9";
    }
    if (titleEl) {
      titleEl.textContent =
        tone === "ok"
          ? `Flow2API v${VERSION} · ${LABEL} 已连接`
          : `Flow2API v${VERSION} · ${LABEL}`;
    }
  }

  function readMusicAuth() {
    const ck = {};
    for (const entry of document.cookie.split("; ")) {
      const idx = entry.indexOf("=");
      if (idx > 0) {
        ck[entry.slice(0, idx)] = decodeURIComponent(entry.slice(idx + 1));
      }
    }
    let raw = "";
    if (ck["sb-sb-auth-token"]) {
      raw = ck["sb-sb-auth-token"];
    } else {
      for (let i = 0; i < 6; i++) {
        const part = ck["sb-sb-auth-token." + i];
        if (!part) break;
        raw += part;
      }
    }
    if (!raw) return null;
    try {
      if (raw.startsWith("base64-")) {
        raw = atob(raw.slice(7).replace(/-/g, "+").replace(/_/g, "/"));
      }
      const parsed = JSON.parse(raw);
      return {
        token: parsed.access_token || null,
        expiresAt: parsed.expires_at || 0,
        email: parsed.user?.email ? String(parsed.user.email).trim().toLowerCase() : null,
      };
    } catch {
      return null;
    }
  }

  function email() {
    if (isMusic) {
      const auth = readMusicAuth();
      if (auth?.email) return auth.email;
      return null;
    }
    try {
      const wizEmail = page.WIZ_global_data && page.WIZ_global_data.oPEP7c;
      if (typeof wizEmail === "string" && /^[^\s@]+@[^\s@]+\.[^\s@]+$/.test(wizEmail.trim())) {
        return wizEmail.trim().toLowerCase();
      }
    } catch {}
    for (const el of document.querySelectorAll("[aria-label]")) {
      const label = el.getAttribute("aria-label") || "";
      if (!/Google Account|Google 账号|Google 帳戶|Google-Konto|Compte Google|Google/i.test(label)) continue;
      const match = label.match(/[A-Za-z0-9._%+-]+@[A-Za-z0-9.-]+\.[A-Za-z]{2,}/);
      if (match) return match[0].toLowerCase();
    }
    return null;
  }

  function localServer(value) {
    const u = new URL(value);
    if (
      u.protocol !== "http:" ||
      !["localhost", "127.0.0.1"].includes(u.hostname) ||
      u.username ||
      u.password ||
      u.search ||
      u.hash ||
      u.pathname !== "/"
    ) {
      throw Error("只支持本机地址，例如 http://127.0.0.1:8000");
    }
    return u.origin;
  }

  function request(server, path, body, token) {
    return new Promise((resolve, reject) =>
      GM_xmlhttpRequest({
        method: "POST",
        url: server + path,
        anonymous: true,
        timeout: 15000,
        headers: {
          "Content-Type": "application/json",
          ...(token ? { Authorization: "Bearer " + token } : {}),
        },
        data: JSON.stringify(body),
        onload(r) {
          let data;
          try {
            data = JSON.parse(r.responseText);
          } catch {
            return reject(Error("本机服务返回无效响应，请确认端口属于 Flow2API"));
          }
          if (r.status < 200 || r.status >= 300) {
            const e = Error(data.error?.message || "连接失败（HTTP " + r.status + "）");
            e.status = r.status;
            reject(e);
          } else {
            resolve(data);
          }
        },
        ontimeout: () => reject(Error("本机连接超时，请确认 Flow2API 已启动")),
        onerror: () => reject(Error("无法连接本机 Flow2API，应用启动后将自动重连")),
      })
    );
  }

  function flowURL(value) {
    const u = new URL(value);
    if (u.origin !== "https://flow.google.com" || u.username || u.password) {
      throw Error("拒绝访问 Flow 以外的地址");
    }
    return u;
  }

  const MUSIC_PATH_ALLOWED =
    /^\/(billing\/(credits|subscription)|conversation(\/create)?|conversations\/[a-f0-9-]{36}(\/running-job)?|audio-create-song-status\/[A-Za-z0-9_-]+)$/;

  async function executeMusic(command) {
    const auth = readMusicAuth();
    if (!auth?.email || !auth.token) {
      throw Error("Flow Music 尚未登录，请刷新页面并登录 Google 账号");
    }
    if (auth.email !== connection.email) {
      throw Error("Flow Music 登录账号已切换，请重新连接");
    }
    const input = command.input || {};
    if (command.operation === "identity") {
      return { email: auth.email };
    }
    if (command.operation === "music_api") {
      const method = String(input.method || "GET").toUpperCase();
      const path = String(input.path || "");
      if (!["GET", "POST"].includes(method)) throw Error("不支持的请求方法");
      if (!MUSIC_PATH_ALLOWED.test(path)) throw Error("拒绝访问未授权的 Flow Music 接口：" + path);
      const response = await page.fetch("/__api" + path, {
        method,
        credentials: "include",
        headers: {
          Authorization: "Bearer " + auth.token,
          "content-type": "application/json",
        },
        ...(input.body == null ? {} : { body: JSON.stringify(input.body) }),
        signal: page.AbortSignal.timeout(45000),
      });
      const text = await response.text();
      let parsed = null;
      try {
        parsed = text ? JSON.parse(text) : null;
      } catch {}
      return { status: response.status, text, json: parsed };
    }
    throw Error("不支持的 Flow Music 浏览器操作");
  }

  async function executeFlow(command) {
    for (let i = 0; i < 60 && !email(); i++) await delay(250);
    if (!email() || email() !== connection.email) {
      throw Error("Flow 登录账号已改变，请重新连接");
    }
    const input = command.input || {};
    switch (command.operation) {
      case "identity":
        return { email: email() };
      case "fetch": {
        const url = flowURL(input.url);
        if (!["GET", "POST"].includes(input.method)) throw Error("不支持的请求方法");
        if (input.method === "POST" && url.pathname !== "/_/AiSandboxAngularFrontend/data/batchexecute") {
          throw Error("拒绝非 Flow RPC 请求");
        }
        if (input.method === "GET" && !/^\/(projects|project\/[a-f0-9-]{36})$/.test(url.pathname)) {
          throw Error("拒绝非 Flow 页面请求");
        }
        const response = await page.fetch(url.href, {
          method: input.method,
          credentials: "include",
          headers: { "Content-Type": input.content_type, "x-same-domain": "1" },
          ...(input.body == null ? {} : { body: input.body }),
          signal: page.AbortSignal.timeout(90000),
        });
        return { status: response.status, text: await response.text() };
      }
      case "navigate": {
        const url = flowURL(input.url);
        if (!/^\/(projects|project\/[a-f0-9-]{36})$/.test(url.pathname) || url.search || url.hash) {
          throw Error("项目地址无效");
        }
        if (location.href !== url.href) {
          connection.result = { id: command.id, value: { navigated: true } };
          save();
          location.assign(url.href);
          return new Promise(() => {});
        }
        return { navigated: true };
      }
      case "captcha": {
        if (!["IMAGE_GENERATION", "VIDEO_GENERATION", "UPLOAD_IMAGE"].includes(input.action)) {
          throw Error("验证码动作无效");
        }
        for (let i = 0; i < 80 && !page.grecaptcha?.enterprise; i++) await delay(250);
        const siteKey = "6LdsFiUsAAAAAIjVDZcuLhaHiDn5nnHVXVRQGeMV";
        const deadline = Date.now() + 45000;
        const captured = page.__flow2apiTrustedRecaptchaExecute;
        const enterprise = page.grecaptcha?.enterprise;
        const actionGuard = !!page.__flow2apiRecaptchaActionGuardInstalled;
        const errors = [];
        const strategies = [];
        let container = null;
        if (typeof captured === "function") {
          strategies.push(["captured", () => captured(siteKey, { action: input.action })]);
        }
        if (enterprise && typeof enterprise.execute === "function") {
          let executeSource = "";
          try {
            executeSource = String(enterprise.execute);
          } catch {}
          if (executeSource.includes("extension_hijack_detected") && !actionGuard) {
            errors.push("action guard 不可用");
          } else {
            strategies.push([
              "direct",
              () => enterprise.execute.call(enterprise, siteKey, { action: input.action }),
            ]);
            if (typeof enterprise.render === "function") {
              strategies.push([
                "render",
                () => {
                  container = document.createElement("div");
                  container.setAttribute("data-flow2api-recaptcha", "1");
                  container.style.display = "none";
                  (document.body || document.documentElement).appendChild(container);
                  const clientId = enterprise.render(container, {
                    sitekey: siteKey,
                    size: "invisible",
                    action: input.action,
                  });
                  return enterprise.execute.call(enterprise, clientId, { action: input.action });
                },
              ]);
            }
          }
        }
        if (!strategies.length) {
          throw Error("Flow 验证码尚未就绪" + (errors.length ? "（" + errors.join(" | ") + "）" : ""));
        }
        const attempts = [];
        let successfulToken = null;
        let resolveSuccess;
        const successPromise = new Promise((resolve) => {
          resolveSuccess = resolve;
        });
        const waitForResult = async (completion, waitMs) => {
          let t;
          try {
            await Promise.race([
              successPromise,
              completion,
              new Promise((resolve) => {
                t = setTimeout(resolve, Math.max(0, waitMs));
              }),
            ]);
          } finally {
            clearTimeout(t);
          }
        };
        try {
          for (let index = 0; index < strategies.length; index += 1) {
            const [label, factory] = strategies[index];
            const remaining = deadline - Date.now();
            if (remaining <= 0) {
              errors.push(label + ": 超时");
              break;
            }
            const attempt = { label, settled: false };
            attempt.promise = Promise.resolve()
              .then(factory)
              .then(
                (token) => {
                  attempt.settled = true;
                  const normalized = typeof token === "string" ? token.trim() : "";
                  if (!normalized) errors.push(label + ": 验证码为空");
                  else if (Date.now() >= deadline) errors.push(label + ": 超时");
                  else if (!successfulToken) {
                    successfulToken = normalized;
                    resolveSuccess(normalized);
                  }
                },
                (err) => {
                  attempt.settled = true;
                  errors.push(label + ": " + (err?.message || String(err)));
                }
              );
            attempts.push(attempt);
            const attemptMs = Math.max(1, Math.floor(remaining / (strategies.length - index)));
            await waitForResult(attempt.promise, attemptMs);
            if (successfulToken) return { token: successfulToken };
          }
          const remaining = deadline - Date.now();
          if (remaining > 0 && attempts.some((a) => !a.settled)) {
            await waitForResult(Promise.all(attempts.map((a) => a.promise)), remaining);
          }
          if (successfulToken) return { token: successfulToken };
          for (const a of attempts) {
            if (!a.settled) errors.push(a.label + ": 超时");
          }
          throw Error("验证码获取失败（" + (errors.join(" | ") || "超时") + "）");
        } finally {
          if (container) container.remove();
        }
      }
      default:
        throw Error("不支持的浏览器操作");
    }
  }

  async function execute(command) {
    return isMusic ? executeMusic(command) : executeFlow(command);
  }

  async function pairNow(silent) {
    if (yieldToExtensionIfActive()) return false;
    if (busy || pairing) {
      if (!silent) setStatus("正在执行操作，请稍候…", "warn");
      return false;
    }
    pairing = true;
    if (connectBtn) connectBtn.disabled = true;
    clearTimeout(timer);
    try {
      const server = localServer(
        (addressInput?.value || connection?.server || GM_getValue(SERVER_KEY, "http://127.0.0.1:8000")).trim()
      );
      GM_setValue(SERVER_KEY, server);
      const currentEmail = email();
      if (!currentEmail) {
        if (!silent) setStatus(`请先在当前页面登录 ${LABEL} 账号`, "warn");
        return false;
      }
      const paired = await request(server, PAIR_PATH, { code: "", email: currentEmail });
      connection = { server, token: paired.token, email: currentEmail, resume: true };
      save();
      stopped = false;
      setStatus(`已连接 ${currentEmail}，正在校验身份与积分…`, "warn");
      poll();
      return true;
    } catch (error) {
      setStatus(error.message || "连接失败", "error");
      if (connection) poll();
      return false;
    } finally {
      pairing = false;
      if (connectBtn) connectBtn.disabled = false;
    }
  }

  async function poll() {
    if (yieldToExtensionIfActive()) return;
    if (!connection || stopped || busy) return;
    busy = true;
    try {
      const response = await request(
        connection.server,
        POLL_PATH,
        { result: connection.result || null, resume: Boolean(connection.resume) },
        connection.token
      );
      connection.result = null;
      connection.resume = false;
      save();
      const state = response.connection || {};
      if (state.state === "ready") {
        setStatus(`已连接 · ${connection.email}\n可返回 Flow2API 创作，请保持此标签页打开。`, "ok");
      } else if (state.state === "failed") {
        setStatus(state.message || "校验失败，请点击重新连接", "error");
      } else {
        setStatus(state.message || "正在校验身份与积分…", "warn");
      }
      for (const command of response.commands || []) {
        try {
          connection.result = { id: command.id, value: await execute(command) };
        } catch (err) {
          connection.result = {
            id: command.id,
            error: err?.message || "浏览器操作失败，请检查登录状态与网络",
          };
        }
        save();
      }
    } catch (error) {
      setStatus(error.message || "连接异常，正在重试…", "error");
      if (error.status === 401) {
        clearConnection();
      }
    } finally {
      busy = false;
      if (!stopped && connection) {
        timer = setTimeout(poll, 500);
      }
    }
  }

  let heartbeatBusy = false;
  setInterval(async () => {
    if (yieldToExtensionIfActive()) return;
    if (!connection || stopped || heartbeatBusy) return;
    heartbeatBusy = true;
    try {
      await request(connection.server, POLL_PATH, { heartbeat: true }, connection.token);
    } catch (err) {
      if (err && err.status === 401) clearConnection();
    } finally {
      heartbeatBusy = false;
    }
  }, 3000);

  // Automatic self-healing connect loop: retries cleanly if desktop app starts later or 401 resets token.
  setInterval(() => {
    if (yieldToExtensionIfActive()) return;
    if (stopped || busy || pairing) return;
    if (!document.getElementById("flow2api-connection") && document.body) {
      mount();
      return;
    }
    if (!connection) {
      if (email()) pairNow(true);
    }
  }, 5000);

  function mount() {
    if (yieldToExtensionIfActive()) return;
    if (!document.body) {
      setTimeout(mount, 250);
      return;
    }
    document.getElementById("flow2api-connection")?.remove();
    const panel = document.createElement("aside");
    panel.id = "flow2api-connection";
    panel.setAttribute("aria-label", `Flow2API v${VERSION} ${LABEL} 直连`);
    panel.style.cssText =
      "position:fixed;bottom:18px;right:18px;z-index:2147483647;background:#1c1d20;color:#ececef;padding:12px 14px;border:1px solid #3b3d43;border-radius:12px;font:12px/1.5 system-ui,-apple-system,sans-serif;width:296px;box-shadow:0 10px 28px rgba(0,0,0,.45)";

    const header = document.createElement("div");
    header.style.cssText = "display:flex;align-items:center;justify-content:space-between;gap:8px";
    const titleWrap = document.createElement("div");
    titleWrap.style.cssText = "display:flex;align-items:center;gap:7px;min-width:0";
    dotEl = document.createElement("span");
    dotEl.style.cssText = "width:8px;height:8px;border-radius:50%;background:#9c9fa9;flex-shrink:0";
    titleEl = document.createElement("strong");
    titleEl.style.cssText = "font-size:12px;font-weight:600;white-space:nowrap;overflow:hidden;text-overflow:ellipsis";
    titleEl.textContent = `Flow2API v${VERSION} · ${LABEL}`;
    titleWrap.append(dotEl, titleEl);

    const toggle = document.createElement("button");
    toggle.type = "button";
    toggle.textContent = "收起";
    toggle.setAttribute("aria-expanded", "true");
    toggle.style.cssText =
      "background:transparent;color:#a6a9b2;border:1px solid #3b3d43;border-radius:6px;padding:3px 9px;min-height:26px;font:11px system-ui;cursor:pointer;flex-shrink:0";
    header.append(titleWrap, toggle);

    const body = document.createElement("div");
    body.style.cssText = "margin-top:10px";

    addressInput = document.createElement("input");
    addressInput.value = connection?.server || GM_getValue(SERVER_KEY, "http://127.0.0.1:8000");
    addressInput.placeholder = "http://127.0.0.1:8000";
    addressInput.setAttribute("aria-label", "Flow2API 本机地址");
    addressInput.style.cssText =
      "display:block;width:100%;box-sizing:border-box;background:#17181a;color:#ececef;border:1px solid #54575f;border-radius:6px;padding:7px 9px;font:12px system-ui;margin-top:6px";

    const advanced = document.createElement("details");
    advanced.style.cssText = "margin:8px 0 0";
    const summary = document.createElement("summary");
    summary.textContent = "本机端口设置";
    summary.style.cssText = "cursor:pointer;color:#a6a9b2;font-size:11px;user-select:none";
    advanced.append(summary, addressInput);

    connectBtn = document.createElement("button");
    connectBtn.type = "button";
    connectBtn.textContent = `重新同步 ${LABEL}`;
    connectBtn.style.cssText =
      "width:100%;padding:8px 10px;background:#c6d6cb;color:#203428;border:0;border-radius:6px;font-weight:600;font-size:12px;cursor:pointer";
    connectBtn.onclick = () => pairNow(false);

    statusEl = document.createElement("div");
    statusEl.setAttribute("role", "status");
    statusEl.setAttribute("aria-live", "polite");
    statusEl.style.cssText =
      "white-space:pre-wrap;overflow-wrap:anywhere;font-size:11.5px;line-height:1.55;margin-top:8px;color:#cbccd1";

    toggle.onclick = () => {
      const hidden = body.style.display === "none";
      body.style.display = hidden ? "block" : "none";
      toggle.textContent = hidden ? "收起" : "展开";
      toggle.setAttribute("aria-expanded", hidden ? "true" : "false");
      panel.style.width = hidden ? "296px" : "auto";
    };

    body.append(connectBtn, statusEl, advanced);
    panel.append(header, body);
    document.body.append(panel);

    setStatus(`等待登录并自动连接本机 Flow2API…`, "idle");
    if (connection) {
      poll();
    } else {
      (async () => {
        for (let i = 0; i < 12 && !connection && !stopped; i++) {
          if (email()) {
            const ok = await pairNow(true);
            if (ok) break;
          }
          await delay(1000);
        }
      })();
    }
  }

  if (document.readyState === "loading") {
    document.addEventListener("DOMContentLoaded", mount, { once: true });
  } else {
    mount();
  }
})();
