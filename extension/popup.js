const $ = (id) => document.getElementById(id);

function setStatus(text, error = false) {
  const node = $("status");
  node.textContent = text;
  node.className = error ? "error" : "";
}

function normalizeBaseUrl(value) {
  return value.trim().replace(/\/+$/, "");
}

async function readEmailFromPage() {
  const [tab] = await chrome.tabs.query({ active: true, currentWindow: true });
  if (!tab?.id) throw new Error("找不到当前标签页");
  const result = await chrome.tabs.sendMessage(tab.id, { type: "flow2api_detect_email" });
  if (!result?.email) throw new Error("页面中未找到邮箱，请手动填写");
  $("email").value = result.email;
}

async function getFlowCookies() {
  // URL filtering includes parent Google cookies and preserves domain/path metadata.
  return (await chrome.cookies.getAll({ url: "https://flow.google.com/" }))
    .filter(cookie => cookie.value && !cookie.partitionKey);

}

async function getAdminSessionToken(baseUrl) {
  const cookie = await chrome.cookies.get({ url: `${baseUrl}/`, name: "admin_session" });
  return cookie?.value || "";
}

async function sync() {
  const email = $("email").value.trim().toLowerCase();
  const baseUrl = normalizeBaseUrl($("serverUrl").value);
  if (!/^[^\s@]+@[^\s@]+\.[^\s@]+$/.test(email)) return setStatus("请输入有效的 Google 账号邮箱。", true);
  let url;
  try { url = new URL(baseUrl); } catch { return setStatus("Flow2API 地址无效。", true); }
  if (!["http:", "https:"].includes(url.protocol)) return setStatus("地址必须使用 http 或 https。", true);
  $("sync").disabled = true;
  setStatus("正在读取 Flow Cookie...");
  try {
    const cookies = await getFlowCookies();
    if (!cookies.length) throw new Error("未读取到 Google Cookie，请确认当前浏览器已登录 Flow。");
    const cookieHeader = JSON.stringify(cookies);
    const adminSession = await getAdminSessionToken(baseUrl);
    if (!adminSession) throw new Error("未找到 Flow2API 管理会话，请先在同一浏览器中登录后台；地址必须与后台登录地址完全一致。");
    const response = await fetch(`${baseUrl}/api/browser-login/sync`, {
      method: "POST",
      credentials: "omit",
      headers: { "Content-Type": "application/json", "Authorization": `Bearer ${adminSession}` },
      body: JSON.stringify({ login_account: email, google_cookies: cookieHeader, protocol_mode: "protocol" })
    });
    const payload = await response.json().catch(() => ({}));
    if (response.status === 401) throw new Error("Flow2API 管理后台未登录或会话已过期，请先打开后台登录。");
    if (!response.ok || !payload.success) {
      const details = Array.isArray(payload.errors) ? `\n${payload.errors.join("\n")}` : "";
      throw new Error((payload.message || payload.detail || `同步失败（HTTP ${response.status}）`) + details);
    }
    setStatus(`同步成功：新增 ${payload.added || 0}，更新 ${payload.updated || 0}`);
  } catch (error) {
    setStatus(error.message || "同步失败", true);
  } finally {
    $("sync").disabled = false;
  }
}

document.addEventListener("DOMContentLoaded", () => {
  chrome.storage.local.get({ flow2apiServerUrl: "http://127.0.0.1:8000", flow2apiEmail: "account1@example.com" }, (settings) => { $("serverUrl").value = settings.flow2apiServerUrl; $("email").value = settings.flow2apiEmail || "account1@example.com"; });
  $("detect").addEventListener("click", () => readEmailFromPage().catch((error) => setStatus(error.message, true)));
  $("sync").addEventListener("click", () => { chrome.storage.local.set({ flow2apiServerUrl: normalizeBaseUrl($("serverUrl").value), flow2apiEmail: $("email").value.trim().toLowerCase() }); sync(); });
});
