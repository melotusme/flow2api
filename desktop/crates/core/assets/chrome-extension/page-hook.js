(function () {
  "use strict";
  if (window.top !== window) return;
  const host = location.hostname;
  const isFlow = host === "flow.google.com";
  const isMusic = host === "www.flowmusic.app" || host === "flowmusic.app";
  if (!isFlow && !isMusic) return;

  if (document.documentElement) {
    document.documentElement.dataset.flow2apiBridgeActive = "1";
  }

  const page = window;

  // Tear down any unconsumed handshake listeners from a previous extension reload on the same tab.
  if (typeof page.__flow2apiExtHookCleanup === "function") {
    try {
      page.__flow2apiExtHookCleanup();
    } catch {}
  }

  // Mark hook installed (configurable so reloads never throw if inspected).
  if (!page.__flow2apiExtPageHookInstalled) {
    try {
      page.Object.defineProperty(page, "__flow2apiExtPageHookInstalled", {
        value: true,
        configurable: true,
      });
    } catch {}
  }

  const nativeFetch = page.fetch.bind(page);
  const nativeSetTimeout = page.setTimeout.bind(page);
  const nativeClearTimeout = page.clearTimeout.bind(page);

  function timeoutSignal(ms) {
    if (page.AbortSignal && typeof page.AbortSignal.timeout === "function") {
      return page.AbortSignal.timeout(ms);
    }
    const ctrl = new AbortController();
    nativeSetTimeout(() => ctrl.abort(), ms);
    return ctrl.signal;
  }

  // Install reCAPTCHA Enterprise hook and action guard on Google Flow (idempotent across reloads).
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

  const delay = (ms) => new Promise((resolve) => nativeSetTimeout(resolve, ms));

  function decodeBase64Utf8(b64) {
    const normalized = b64.replace(/-/g, "+").replace(/_/g, "/");
    const pad = normalized.length % 4;
    const padded = pad ? normalized + "=".repeat(4 - pad) : normalized;
    const bin = atob(padded);
    const bytes = new Uint8Array(bin.length);
    for (let i = 0; i < bin.length; i++) {
      bytes[i] = bin.charCodeAt(i);
    }
    return new TextDecoder().decode(bytes);
  }

  function readMusicAuth() {
    const ck = {};
    for (const entry of document.cookie.split("; ")) {
      const idx = entry.indexOf("=");
      if (idx > 0) {
        try {
          ck[entry.slice(0, idx)] = decodeURIComponent(entry.slice(idx + 1));
        } catch {
          ck[entry.slice(0, idx)] = entry.slice(idx + 1);
        }
      }
    }
    const baseCandidates = [];
    if (ck["sb-sb-auth-token"] || ck["sb-sb-auth-token.0"]) {
      baseCandidates.push("sb-sb-auth-token");
    }
    for (const key of Object.keys(ck)) {
      const m = key.match(/^(sb-[A-Za-z0-9_-]+-auth-token)(?:\.0)?$/);
      if (m && !baseCandidates.includes(m[1])) {
        baseCandidates.push(m[1]);
      }
    }
    for (const base of baseCandidates) {
      let raw = "";
      if (ck[base]) {
        raw = ck[base];
      } else {
        for (let i = 0; i < 20; i++) {
          const part = ck[base + "." + i];
          if (!part) break;
          raw += part;
        }
      }
      if (!raw) continue;
      try {
        const jsonText = raw.startsWith("base64-")
          ? decodeBase64Utf8(raw.slice(7))
          : raw;
        const parsed = JSON.parse(jsonText);
        if (parsed && (parsed.access_token || parsed.user?.email)) {
          return {
            token: parsed.access_token || null,
            expiresAt: parsed.expires_at || 0,
            email: parsed.user?.email ? String(parsed.user.email).trim().toLowerCase() : null,
          };
        }
      } catch {}
    }
    return null;
  }

  function detectEmail() {
    if (isMusic) {
      const auth = readMusicAuth();
      return auth?.email || null;
    }
    try {
      const wizEmail = page.WIZ_global_data && page.WIZ_global_data.oPEP7c;
      if (typeof wizEmail === "string" && /^[^\s@]+@[^\s@]+\.[^\s@]+$/.test(wizEmail.trim())) {
        return wizEmail.trim().toLowerCase();
      }
    } catch {}
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

  function flowURL(value) {
    const u = new URL(value);
    if (u.origin !== "https://flow.google.com" || u.username || u.password) {
      throw new Error("拒绝访问 Flow 以外的地址");
    }
    return u;
  }

  const MUSIC_PATH_ALLOWED =
    /^\/(billing\/(credits|subscription)|conversation(\/create)?|conversations\/[a-f0-9-]{36}(\/running-job)?|audio-create-song-status\/[A-Za-z0-9_-]+)$/;

  async function executeMusic(operation, expectedEmail, input) {
    const payload = input || {};
    const fallbackAuth =
      payload.__fallbackAuth && typeof payload.__fallbackAuth === "object"
        ? payload.__fallbackAuth
        : null;
    const domAuth = readMusicAuth();
    const auth =
      domAuth?.email && domAuth?.token
        ? domAuth
        : fallbackAuth?.email && fallbackAuth?.token
          ? fallbackAuth
          : domAuth || fallbackAuth;

    if (!auth?.email || !auth.token) {
      throw new Error("Flow Music 尚未登录，请刷新页面并登录 Google 账号");
    }
    if (auth.email !== expectedEmail) {
      throw new Error("Flow Music 登录账号已切换，请重新连接");
    }
    if (operation === "identity") {
      return { email: auth.email };
    }
    if (operation === "music_api") {
      const method = String(payload.method || "GET").toUpperCase();
      const path = String(payload.path || "");
      if (!["GET", "POST"].includes(method)) {
        throw new Error("不支持的请求方法");
      }
      if (!MUSIC_PATH_ALLOWED.test(path)) {
        throw new Error("拒绝访问未授权的 Flow Music 接口：" + path);
      }
      const response = await nativeFetch("/__api" + path, {
        method,
        credentials: "include",
        headers: {
          Authorization: "Bearer " + auth.token,
          "content-type": "application/json",
        },
        ...(payload.body == null ? {} : { body: JSON.stringify(payload.body) }),
        signal: timeoutSignal(45000),
      });
      const text = await response.text();
      let parsed = null;
      try {
        parsed = text ? JSON.parse(text) : null;
      } catch {}
      return { status: response.status, text, json: parsed };
    }
    throw new Error("不支持的 Flow Music 浏览器操作");
  }

  async function executeFlow(operation, expectedEmail, input) {
    const payload = input || {};
    if (operation === "perform_navigate") {
      const url = flowURL(payload.url);
      if (!/^\/(projects|project\/[a-f0-9-]{36})$/.test(url.pathname) || url.search || url.hash) {
        throw new Error("项目地址无效");
      }
      location.assign(url.href);
      return { navigated: true };
    }

    for (let i = 0; i < 60 && !detectEmail(); i++) {
      await delay(250);
    }
    const currentEmail = detectEmail();
    if (!currentEmail || currentEmail !== expectedEmail) {
      throw new Error("Flow 登录账号已改变，请重新连接");
    }

    switch (operation) {
      case "identity":
        return { email: currentEmail };
      case "fetch": {
        const url = flowURL(payload.url);
        if (!["GET", "POST"].includes(payload.method)) {
          throw new Error("不支持的请求方法");
        }
        if (payload.method === "POST" && url.pathname !== "/_/AiSandboxAngularFrontend/data/batchexecute") {
          throw new Error("拒绝非 Flow RPC 请求");
        }
        if (payload.method === "GET" && !/^\/(projects|project\/[a-f0-9-]{36})$/.test(url.pathname)) {
          throw new Error("拒绝非 Flow 页面请求");
        }
        const response = await nativeFetch(url.href, {
          method: payload.method,
          credentials: "include",
          headers: {
            "Content-Type": payload.content_type,
            "x-same-domain": "1",
          },
          ...(payload.body == null ? {} : { body: payload.body }),
          signal: timeoutSignal(90000),
        });
        return { status: response.status, text: await response.text() };
      }
      case "navigate": {
        const url = flowURL(payload.url);
        if (!/^\/(projects|project\/[a-f0-9-]{36})$/.test(url.pathname) || url.search || url.hash) {
          throw new Error("项目地址无效");
        }
        if (location.href !== url.href) {
          return { __willNavigateTo: url.href };
        }
        return { navigated: true };
      }
      case "captcha": {
        if (!["IMAGE_GENERATION", "VIDEO_GENERATION", "UPLOAD_IMAGE"].includes(payload.action)) {
          throw new Error("验证码动作无效");
        }
        for (let i = 0; i < 80 && !page.grecaptcha?.enterprise; i++) {
          await delay(250);
        }
        const siteKey = "6LdsFiUsAAAAAIjVDZcuLhaHiDn5nnHVXVRQGeMV";
        const deadline = Date.now() + 45000;
        const captured = page.__flow2apiTrustedRecaptchaExecute;
        const enterprise = page.grecaptcha?.enterprise;
        const actionGuard = !!page.__flow2apiRecaptchaActionGuardInstalled;
        const errors = [];
        const strategies = [];
        let container = null;
        if (typeof captured === "function") {
          strategies.push(["captured", () => captured(siteKey, { action: payload.action })]);
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
              () => enterprise.execute.call(enterprise, siteKey, { action: payload.action }),
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
                    action: payload.action,
                  });
                  return enterprise.execute.call(enterprise, clientId, { action: payload.action });
                },
              ]);
            }
          }
        }
        if (!strategies.length) {
          throw new Error("Flow 验证码尚未就绪" + (errors.length ? "（" + errors.join(" | ") + "）" : ""));
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
                t = nativeSetTimeout(resolve, Math.max(0, waitMs));
              }),
            ]);
          } finally {
            nativeClearTimeout(t);
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
          throw new Error("验证码获取失败（" + (errors.join(" | ") || "超时") + "）");
        } finally {
          if (container) container.remove();
        }
      }
      default:
        throw new Error("不支持的浏览器操作");
    }
  }

  // Negotiate a locked, private MessagePort with content.js (works both at document_start and on retroactive injection).
  // Localhost bridge tokens never enter MAIN world; untrusted page scripts cannot read or forge messages.
  let channelSecret = null;
  let port = null;

  async function handlePortRequest(msg) {
    if (!msg || !port || msg.secret !== channelSecret || !msg.id) return;
    try {
      let value;
      if (msg.operation === "probe_email") {
        value = { email: detectEmail() };
      } else if (isMusic) {
        value = await executeMusic(msg.operation, msg.expectedEmail, msg.input);
      } else {
        value = await executeFlow(msg.operation, msg.expectedEmail, msg.input);
      }
      port.postMessage({ id: msg.id, secret: channelSecret, ok: true, value });
    } catch (err) {
      port.postMessage({
        id: msg.id,
        secret: channelSecret,
        ok: false,
        error: err?.message || "浏览器操作失败，请检查登录状态与网络",
      });
    }
  }

  function onSeed(ev) {
    ev.stopImmediatePropagation();
    if (channelSecret !== null || port !== null) return;
    const root = document.documentElement;
    const seed = root ? root.getAttribute("data-flow2api-seed") : null;
    if (seed && seed.length >= 32) {
      root.removeAttribute("data-flow2api-seed");
      channelSecret = seed;
      window.removeEventListener("__flow2api_ext_seed__", onSeed, true);
    }
  }

  function onPortTransfer(ev) {
    if (port !== null || !channelSecret || ev.source !== window) return;
    if (!ev.data || ev.data.__flow2api_port_init__ !== channelSecret || !ev.ports || !ev.ports[0]) {
      return;
    }
    ev.stopImmediatePropagation();
    window.removeEventListener("message", onPortTransfer, true);
    port = ev.ports[0];
    port.onmessage = (msgEvent) => {
      handlePortRequest(msgEvent.data);
    };
    port.postMessage({ type: "ready", secret: channelSecret });
  }

  try {
    page.Object.defineProperty(page, "__flow2apiExtHookCleanup", {
      value: () => {
        window.removeEventListener("__flow2api_ext_seed__", onSeed, true);
        window.removeEventListener("message", onPortTransfer, true);
      },
      configurable: true,
    });
  } catch {}

  window.addEventListener("__flow2api_ext_seed__", onSeed, true);
  window.addEventListener("message", onPortTransfer, true);
  window.dispatchEvent(new Event("__flow2api_ext_hook_ready__"));
})();
