"use strict";

const ALLOWED_PATHS = new Set([
  "/api/browser-bridge/pair",
  "/api/browser-bridge/poll",
  "/api/music-bridge/pair",
  "/api/music-bridge/poll",
]);

const ALLOWED_PAGE_HOSTS = new Set([
  "flow.google.com",
  "www.flowmusic.app",
  "flowmusic.app",
]);

const MUSIC_HOSTS = new Set([
  "www.flowmusic.app",
  "flowmusic.app",
]);

const TARGET_URL_PATTERNS = [
  "https://flow.google.com/*",
  "https://www.flowmusic.app/*",
  "https://flowmusic.app/*",
];

function normalizeLocalServer(value) {
  let u;
  try {
    u = new URL(String(value || ""));
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

function parseSupabaseAuthMap(ck) {
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
    for (const candidate of raw.includes("%") ? [safeDecodeURIComponent(raw), raw] : [raw]) {
      if (!candidate) continue;
      try {
        const jsonText = candidate.startsWith("base64-")
          ? decodeBase64Utf8(candidate.slice(7))
          : candidate;
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
  }
  return null;
}

function safeDecodeURIComponent(value) {
  try {
    return decodeURIComponent(value);
  } catch {
    return null;
  }
}

async function handleMusicCookieAuth(sender) {
  if (sender?.id !== chrome.runtime.id) {
    throw new Error("拒绝非本扩展请求");
  }
  let pageUrl;
  try {
    pageUrl = new URL(String(sender?.url || ""));
  } catch {
    throw new Error("来源页面地址无效");
  }
  if (pageUrl.protocol !== "https:" || !MUSIC_HOSTS.has(pageUrl.hostname)) {
    throw new Error("拒绝非 Flow Music 页面读取认证状态");
  }
  if (!chrome.cookies?.getAll) {
    return null;
  }
  const cookies = await chrome.cookies.getAll({ domain: "flowmusic.app" });
  const ck = {};
  for (const c of cookies || []) {
    if (c && typeof c.name === "string" && typeof c.value === "string") {
      ck[c.name] = c.value;
    }
  }
  return parseSupabaseAuthMap(ck);
}

async function handleBridgeRequest(message, sender) {
  if (sender?.id !== chrome.runtime.id) {
    throw new Error("拒绝非本扩展请求");
  }
  if (sender?.url) {
    let pageUrl;
    try {
      pageUrl = new URL(sender.url);
    } catch {
      throw new Error("来源页面地址无效");
    }
    if (pageUrl.protocol !== "https:" || !ALLOWED_PAGE_HOSTS.has(pageUrl.hostname)) {
      throw new Error("拒绝非授权页面调用本机桥接");
    }
  }

  const origin = normalizeLocalServer(message.server);
  const path = String(message.path || "");
  if (!ALLOWED_PATHS.has(path)) {
    throw new Error("拒绝访问未授权的本机桥接路径");
  }

  const controller = new AbortController();
  const timeoutId = setTimeout(() => controller.abort(), 15000);
  let response;
  try {
    response = await fetch(origin + path, {
      method: "POST",
      credentials: "omit",
      cache: "no-store",
      headers: {
        "Content-Type": "application/json",
        ...(message.token ? { Authorization: "Bearer " + String(message.token) } : {}),
      },
      body: JSON.stringify(message.body ?? {}),
      signal: controller.signal,
    });
  } catch (err) {
    if (err && (err.name === "AbortError" || err.name === "TimeoutError")) {
      throw new Error("本机连接超时，请确认 Flow2API 已启动");
    }
    throw new Error("无法连接本机 Flow2API，应用启动后将自动重连");
  } finally {
    clearTimeout(timeoutId);
  }

  const rawText = await response.text();
  let data;
  try {
    data = JSON.parse(rawText);
  } catch {
    throw new Error("本机服务返回无效响应，请确认端口属于 Flow2API");
  }

  if (response.status < 200 || response.status >= 300) {
    const err = new Error(data?.error?.message || "连接失败（HTTP " + response.status + "）");
    err.status = response.status;
    throw err;
  }

  return data;
}

// Retroactively inject into already-open Flow / Flow Music tabs on install, reload, or browser startup
// so users never need to manually refresh existing tabs after installing or updating the extension.
let injectSweepPromise = null;

async function injectIntoTabIfNeeded(tabId) {
  if (!chrome.scripting?.executeScript) return;
  try {
    const probe = await chrome.scripting.executeScript({
      target: { tabId },
      func: () =>
        Boolean(typeof window.__flow2apiExtIsAlive === "function" && window.__flow2apiExtIsAlive()),
    });
    if (probe && probe[0] && probe[0].result === true) {
      return;
    }
    await chrome.scripting.executeScript({
      target: { tabId },
      files: ["page-hook.js"],
      world: "MAIN",
      injectImmediately: true,
    });
    await chrome.scripting.executeScript({
      target: { tabId },
      files: ["content.js"],
      injectImmediately: true,
    });
  } catch {}
}

function sweepExistingTabs() {
  if (injectSweepPromise) return injectSweepPromise;
  if (!chrome.tabs?.query || !chrome.scripting?.executeScript) {
    return Promise.resolve();
  }
  injectSweepPromise = (async () => {
    try {
      const tabs = await chrome.tabs.query({ url: TARGET_URL_PATTERNS });
      for (const tab of tabs || []) {
        if (typeof tab?.id === "number") {
          await injectIntoTabIfNeeded(tab.id);
        }
      }
    } catch {}
  })().finally(() => {
    injectSweepPromise = null;
  });
  return injectSweepPromise;
}

if (chrome.runtime?.onInstalled) {
  chrome.runtime.onInstalled.addListener(() => {
    sweepExistingTabs();
  });
}

if (chrome.runtime?.onStartup) {
  chrome.runtime.onStartup.addListener(() => {
    sweepExistingTabs();
  });
}

sweepExistingTabs();

// Drive unthrottled poll ticks to connected content scripts so background/minimized
// Flow and Flow Music tabs never stall on Chrome's 60s Intensive Timer Throttling.
const activePorts = new Set();
let tickInterval = null;

function ensureTickLoop() {
  if (tickInterval !== null || activePorts.size === 0) return;
  tickInterval = setInterval(() => {
    if (activePorts.size === 0) {
      clearInterval(tickInterval);
      tickInterval = null;
      return;
    }
    for (const port of activePorts) {
      try {
        port.postMessage({ type: "tick" });
      } catch {
        activePorts.delete(port);
      }
    }
  }, 1500);
}

if (chrome.runtime?.onConnect) {
  chrome.runtime.onConnect.addListener((port) => {
    if (!port || port.name !== "flow2api-tick" || port.sender?.id !== chrome.runtime.id) {
      return;
    }
    activePorts.add(port);
    ensureTickLoop();
    port.onDisconnect.addListener(() => {
      activePorts.delete(port);
      if (activePorts.size === 0 && tickInterval !== null) {
        clearInterval(tickInterval);
        tickInterval = null;
      }
    });
  });
}

chrome.runtime.onMessage.addListener((message, sender, sendResponse) => {
  if (!message || typeof message.type !== "string") {
    return false;
  }
  if (message.type === "flow2api_music_auth") {
    handleMusicCookieAuth(sender).then(
      (auth) => sendResponse({ ok: true, auth }),
      (err) => sendResponse({ ok: false, error: err?.message || "读取认证状态失败" })
    );
    return true;
  }
  if (message.type === "flow2api_bridge_request") {
    handleBridgeRequest(message, sender).then(
      (data) => sendResponse({ ok: true, data }),
      (err) =>
        sendResponse({
          ok: false,
          status: err?.status || 0,
          error: err?.message || "无法连接本机 Flow2API，应用启动后将自动重连",
        })
    );
    return true;
  }
  return false;
});
