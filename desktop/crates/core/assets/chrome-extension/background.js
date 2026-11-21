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

chrome.runtime.onMessage.addListener((message, sender, sendResponse) => {
  if (!message || message.type !== "flow2api_bridge_request") {
    return false;
  }
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
});
