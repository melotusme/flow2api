(function () {
  "use strict";
  if (window.top !== window) return;
  const host = location.hostname;
  const isFlow = host === "flow.google.com";
  const isMusic = host === "www.flowmusic.app" || host === "flowmusic.app";
  if (!isFlow && !isMusic) return;
  if (window.__flow2apiExtContentActive) return;
  window.__flow2apiExtContentActive = true;

  function claimBridgeOwnership() {
    if (document.documentElement) {
      document.documentElement.dataset.flow2apiBridgeActive = "1";
    }
  }
  claimBridgeOwnership();

  const VERSION =
    (typeof chrome !== "undefined" && chrome.runtime?.getManifest?.()?.version) || "0.4.0";
  const SERVICE = isMusic ? "music" : "flow";
  const PAIR_PATH = isMusic ? "/api/music-bridge/pair" : "/api/browser-bridge/pair";
  const POLL_PATH = isMusic ? "/api/music-bridge/poll" : "/api/browser-bridge/poll";
  const LABEL = isMusic ? "Flow Music" : "Flow";
  const KEY = "flow2api.connection.v2." + SERVICE;
  const SERVER_KEY = "flow2api.connection.v2.server";
  const DEFAULT_SERVER = "http://127.0.0.1:8000";

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
  const CONN_STORAGE_KEY = KEY + "." + tabId;

  // Private MessageChannel handshake with MAIN-world page-hook.js.
  let channelSecret = null;
  let hookPort = null;
  let resolveHookReady = null;
  const hookReadyPromise = new Promise((resolve) => {
    resolveHookReady = resolve;
  });
  const pendingHookCalls = new Map();

  function establishHookChannel() {
    if (channelSecret || !document.documentElement) return false;
    const candidate = crypto.randomUUID() + "-" + crypto.randomUUID();
    document.documentElement.setAttribute("data-flow2api-seed", candidate);
    window.dispatchEvent(new Event("__flow2api_ext_seed__"));
    if (document.documentElement.hasAttribute("data-flow2api-seed")) {
      document.documentElement.removeAttribute("data-flow2api-seed");
      return false;
    }
    channelSecret = candidate;
    const mc = new MessageChannel();
    mc.port1.onmessage = (ev) => {
      const msg = ev.data;
      if (!msg || msg.secret !== channelSecret) return;
      if (msg.type === "ready") {
        hookPort = mc.port1;
        resolveHookReady(mc.port1);
        return;
      }
      const entry = pendingHookCalls.get(msg.id);
      if (!entry) return;
      pendingHookCalls.delete(msg.id);
      clearTimeout(entry.timer);
      if (msg.ok) {
        entry.resolve(msg.value);
      } else {
        entry.reject(new Error(msg.error || "浏览器操作失败，请检查登录状态与网络"));
      }
    };
    window.postMessage({ __flow2api_port_init__: channelSecret }, location.origin, [mc.port2]);
    return true;
  }

  if (!establishHookChannel()) {
    window.addEventListener(
      "__flow2api_ext_hook_ready__",
      () => {
        establishHookChannel();
      },
      { capture: true, once: true }
    );
  }

  async function callPageHook(operation, expectedEmail, input, timeoutMs = 105000) {
    const port = hookPort || (await Promise.race([
      hookReadyPromise,
      new Promise((_, reject) =>
        setTimeout(() => reject(new Error("页面桥接初始化超时，请刷新当前标签页")), 5000)
      ),
    ]));
    const id = crypto.randomUUID();
    return new Promise((resolve, reject) => {
      const timer = setTimeout(() => {
        pendingHookCalls.delete(id);
        reject(new Error("页面操作响应超时，请检查页面状态后重试"));
      }, timeoutMs);
      pendingHookCalls.set(id, { resolve, reject, timer });
      port.postMessage({
        id,
        secret: channelSecret,
        operation,
        expectedEmail,
        input: input ?? null,
      });
    });
  }

  let connection = null;
  let savedServer = DEFAULT_SERVER;
  let stopped = false;
  let busy = false;
  let pairing = false;
  let timer = null;
  let heartbeatInterval = null;
  let healInterval = null;
  let statusEl = null;
  let dotEl = null;
  let titleEl = null;
  let addressInput = null;
  let connectBtn = null;

  const delay = (ms) => new Promise((resolve) => setTimeout(resolve, ms));

  function extensionAlive() {
    return typeof chrome !== "undefined" && Boolean(chrome.runtime?.id);
  }

  function handleInvalidatedExtension() {
    stopped = true;
    clearTimeout(timer);
    if (heartbeatInterval) clearInterval(heartbeatInterval);
    if (healInterval) clearInterval(healInterval);
    setStatus("扩展已更新或重载，请刷新此页面以恢复直连", "warn");
  }

  const storageReady = (async () => {
    if (!extensionAlive()) return;
    try {
      const stored = await chrome.storage.local.get(null);
      if (typeof stored[SERVER_KEY] === "string" && stored[SERVER_KEY]) {
        savedServer = stored[SERVER_KEY];
      }
      if (stored[CONN_STORAGE_KEY] && typeof stored[CONN_STORAGE_KEY] === "object") {
        connection = { ...stored[CONN_STORAGE_KEY], resume: true };
      }
      // Opportunistically prune stale closed-tab entries older than 7 days.
      const staleCutoff = Date.now() - 7 * 24 * 60 * 60 * 1000;
      const staleKeys = [];
      for (const [k, v] of Object.entries(stored)) {
        if (
          k !== CONN_STORAGE_KEY &&
          k !== SERVER_KEY &&
          k.startsWith("flow2api.connection.v2.") &&
          v &&
          typeof v === "object" &&
          typeof v.updatedAt === "number" &&
          v.updatedAt < staleCutoff
        ) {
          staleKeys.push(k);
        }
      }
      if (staleKeys.length > 0) {
        await chrome.storage.local.remove(staleKeys);
      }
    } catch {}
  })();

  async function save() {
    if (!extensionAlive() || !connection) return;
    try {
      await chrome.storage.local.set({
        [CONN_STORAGE_KEY]: { ...connection, updatedAt: Date.now() },
      });
    } catch {
      if (!extensionAlive()) handleInvalidatedExtension();
    }
  }

  async function saveServer(server) {
    savedServer = server;
    if (!extensionAlive()) return;
    try {
      await chrome.storage.local.set({ [SERVER_KEY]: server });
    } catch {
      if (!extensionAlive()) handleInvalidatedExtension();
    }
  }

  async function clearConnection() {
    connection = null;
    if (!extensionAlive()) return;
    try {
      await chrome.storage.local.remove(CONN_STORAGE_KEY);
    } catch {
      if (!extensionAlive()) handleInvalidatedExtension();
    }
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

  function fallbackDomEmail() {
    if (isMusic) {
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
        return parsed.user?.email ? String(parsed.user.email).trim().toLowerCase() : null;
      } catch {
        return null;
      }
    }
    for (const el of document.querySelectorAll("[aria-label]")) {
      const label = el.getAttribute("aria-label") || "";
      if (!/Google Account|Google 账号|Google 帳戶|Google-Konto|Compte Google|Google/i.test(label)) {
        continue;
      }
      const match = label.match(/[A-Za-z0-9._%+-]+@[A-Za-z0-9.-]+\.[A-Za-z]{2,}/);
      if (match) return match[0].toLowerCase();
    }
    return null;
  }

  async function detectEmail() {
    try {
      const probed = await callPageHook("probe_email", null, null, 3000);
      if (probed?.email) return probed.email;
    } catch {}
    return fallbackDomEmail();
  }

  function localServer(value) {
    let u;
    try {
      u = new URL(value);
    } catch {
      throw new Error("只支持本机地址，例如 http://127.0.0.1:8000");
    }
    if (
      u.protocol !== "http:" ||
      !["localhost", "127.0.0.1"].includes(u.hostname) ||
      u.username ||
      u.password ||
      u.search ||
      u.hash ||
      u.pathname !== "/"
    ) {
      throw new Error("只支持本机地址，例如 http://127.0.0.1:8000");
    }
    return u.origin;
  }

  async function request(server, path, body, token) {
    if (!extensionAlive()) {
      handleInvalidatedExtension();
      throw new Error("扩展已更新或重载，请刷新此页面以恢复直连");
    }
    let response;
    try {
      response = await chrome.runtime.sendMessage({
        type: "flow2api_bridge_request",
        server,
        path,
        body,
        token: token || null,
      });
    } catch {
      if (!extensionAlive()) {
        handleInvalidatedExtension();
        throw new Error("扩展已更新或重载，请刷新此页面以恢复直连");
      }
      throw new Error("无法连接本机 Flow2API，应用启动后将自动重连");
    }
    if (!response || !response.ok) {
      const err = new Error(response?.error || "无法连接本机 Flow2API，应用启动后将自动重连");
      if (response?.status) err.status = response.status;
      throw err;
    }
    return response.data;
  }

  async function execute(command) {
    const value = await callPageHook(command.operation, connection.email, command.input || {});
    if (value && typeof value === "object" && value.__willNavigateTo) {
      connection.result = { id: command.id, value: { navigated: true } };
      await save();
      await callPageHook("perform_navigate", connection.email, { url: value.__willNavigateTo }, 5000);
      return new Promise(() => {});
    }
    return value;
  }

  async function pairNow(silent) {
    await storageReady;
    if (stopped) return false;
    if (busy || pairing) {
      if (!silent) setStatus("正在执行操作，请稍候…", "warn");
      return false;
    }
    pairing = true;
    if (connectBtn) connectBtn.disabled = true;
    clearTimeout(timer);
    try {
      const server = localServer(
        (addressInput?.value || connection?.server || savedServer || DEFAULT_SERVER).trim()
      );
      await saveServer(server);
      const currentEmail = await detectEmail();
      if (!currentEmail) {
        if (!silent) setStatus(`请先在当前页面登录 ${LABEL} 账号`, "warn");
        return false;
      }
      const paired = await request(server, PAIR_PATH, { code: "", email: currentEmail });
      connection = { server, token: paired.token, email: currentEmail, resume: true };
      await save();
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
    await storageReady;
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
      await save();
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
        await save();
      }
    } catch (error) {
      setStatus(error.message || "连接异常，正在重试…", "error");
      if (error.status === 401) {
        await clearConnection();
      }
    } finally {
      busy = false;
      if (!stopped && connection) {
        timer = setTimeout(poll, 500);
      }
    }
  }

  let heartbeatBusy = false;
  heartbeatInterval = setInterval(async () => {
    if (!connection || stopped || heartbeatBusy) return;
    heartbeatBusy = true;
    try {
      await request(connection.server, POLL_PATH, { heartbeat: true }, connection.token);
    } catch (err) {
      if (err && err.status === 401) {
        await clearConnection();
      }
    } finally {
      heartbeatBusy = false;
    }
  }, 3000);

  // Automatic self-healing connect loop: retries cleanly if desktop app starts later or 401 resets token.
  healInterval = setInterval(async () => {
    claimBridgeOwnership();
    if (stopped || busy || pairing) return;
    if (!document.getElementById("flow2api-connection") && document.body) {
      mount();
      return;
    }
    if (!connection) {
      const currentEmail = await detectEmail();
      if (currentEmail) pairNow(true);
    }
  }, 5000);

  async function mount() {
    claimBridgeOwnership();
    if (!document.body) {
      setTimeout(mount, 250);
      return;
    }
    await storageReady;
    if (stopped) return;
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
    titleEl.style.cssText =
      "font-size:12px;font-weight:600;white-space:nowrap;overflow:hidden;text-overflow:ellipsis";
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
    addressInput.value = connection?.server || savedServer || DEFAULT_SERVER;
    addressInput.placeholder = DEFAULT_SERVER;
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

    setStatus("等待登录并自动连接本机 Flow2API…", "idle");
    if (connection) {
      poll();
    } else {
      (async () => {
        for (let i = 0; i < 12 && !connection && !stopped; i++) {
          const currentEmail = await detectEmail();
          if (currentEmail) {
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
