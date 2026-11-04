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
  if (!result?.email) throw new Error("未能从当前页面中读取到邮箱，请手动填写或保持留空自动识别。");
  $("email").value = result.email;
  setStatus(`已读取邮箱: ${result.email}`);
}

async function getFlowCookies() {
  // Query cookies across google.com and flow.google.com
  const cookies = await chrome.cookies.getAll({ url: "https://flow.google.com/" });
  return cookies.filter(cookie => cookie.value && !cookie.partitionKey);
}

async function getAdminSessionToken(baseUrl) {
  try {
    const cookie = await chrome.cookies.get({ url: `${baseUrl}/`, name: "admin_session" });
    return cookie?.value || "";
  } catch {
    return "";
  }
}

async function sync() {
  const email = $("email").value.trim().toLowerCase();
  const baseUrl = normalizeBaseUrl($("serverUrl").value);
  if (email && !/^[^\s@]+@[^\s@]+\.[^\s@]+$/.test(email)) {
    return setStatus("邮箱格式不正确，留空则自动从 Flow 页面识别。", true);
  }
  let url;
  try {
    url = new URL(baseUrl);
  } catch {
    return setStatus("Flow2API 地址无效，例如 http://127.0.0.1:8000", true);
  }
  if (!["http:", "https:"].includes(url.protocol)) {
    return setStatus("地址必须使用 http 或 https。", true);
  }

  $("sync").disabled = true;
  setStatus("正在读取当前浏览器的 Flow Cookie 并导入桌面版...");

  try {
    const cookies = await getFlowCookies();
    if (!cookies.length) {
      throw new Error("未读取到 Google Cookie，请确认当前标签页已在 flow.google.com 登录。");
    }

    const cookieHeader = JSON.stringify(cookies);
    const adminSession = await getAdminSessionToken(baseUrl);
    const headers = { "Content-Type": "application/json" };
    if (adminSession) {
      headers["Authorization"] = `Bearer ${adminSession}`;
    }

    const response = await fetch(`${baseUrl}/api/browser-login/sync`, {
      method: "POST",
      credentials: "omit",
      headers,
      body: JSON.stringify({
        login_account: email,
        google_cookies: cookieHeader,
        protocol_mode: "protocol"
      })
    });

    const payload = await response.json().catch(() => ({}));
    if (response.status === 401) {
      throw new Error("Flow2API 鉴权失败，请检查 API Key 或桌面端运行状态。");
    }
    if (!response.ok || !payload.success) {
      const details = Array.isArray(payload.errors) ? `\n${payload.errors.join("\n")}` : "";
      throw new Error((payload.message || payload.detail || payload.error?.message || `同步失败（HTTP ${response.status}）`) + details);
    }

    const accountName = payload.email || email || "Google 账号";
    const creditsStr = (payload.credits !== undefined && payload.credits !== null) ? ` · 余额: ${payload.credits}` : "";
    setStatus(`✓ 同步成功！\n已成功连接 ${accountName}${creditsStr}\n请返回 Flow2API 桌面版查看。`);
  } catch (error) {
    setStatus(error.message || "同步失败，请确认 Flow2API 桌面版已启动。", true);
  } finally {
    $("sync").disabled = false;
  }
}

document.addEventListener("DOMContentLoaded", () => {
  chrome.storage.local.get({ flow2apiServerUrl: "http://127.0.0.1:8000", flow2apiEmail: "" }, (settings) => {
    $("serverUrl").value = settings.flow2apiServerUrl || "http://127.0.0.1:8000";
    if (settings.flow2apiEmail) {
      $("email").value = settings.flow2apiEmail;
    }
  });

  $("detect").addEventListener("click", () => readEmailFromPage().catch((error) => setStatus(error.message, true)));
  $("sync").addEventListener("click", () => {
    chrome.storage.local.set({
      flow2apiServerUrl: normalizeBaseUrl($("serverUrl").value),
      flow2apiEmail: $("email").value.trim().toLowerCase()
    });
    sync();
  });
});
