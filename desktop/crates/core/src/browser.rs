use anyhow::{bail, Context, Result};
use futures_util::{SinkExt, StreamExt};
use serde_json::{json, Value};
use std::{
    collections::HashMap,
    path::{Path, PathBuf},
    process::Stdio,
    sync::Arc,
    time::{Duration, SystemTime},
};
use tokio::{
    process::{Child, Command},
    sync::Mutex,
    time::{sleep, timeout},
};
use tokio_tungstenite::{connect_async, tungstenite::Message, MaybeTlsStream, WebSocketStream};

type Socket = WebSocketStream<MaybeTlsStream<tokio::net::TcpStream>>;
pub struct Session {
    child: Child,
    socket: Socket,
    sequence: u64,
}
pub struct BrowserPool {
    root: PathBuf,
    sessions: Mutex<HashMap<String, Arc<Mutex<Session>>>>,
}

pub fn executable(configured: &str) -> Result<PathBuf> {
    let configured = if configured.trim().is_empty() {
        std::env::var("BROWSER_EXECUTABLE_PATH").unwrap_or_default()
    } else {
        configured.into()
    };
    if !configured.trim().is_empty() {
        let mut path = PathBuf::from(configured.trim());
        if cfg!(target_os = "macos") && path.extension().is_some_and(|s| s == "app") {
            let name = match path.file_name().and_then(|s| s.to_str()) {
                Some("Google Chrome.app") => "Google Chrome",
                _ => bail!("请选择 Google Chrome.app，或直接选择 Chrome 可执行文件"),
            };
            path = path.join("Contents/MacOS").join(name);
        }
        if !path.is_file() {
            bail!("指定的 Chrome 可执行文件不存在");
        }
        return Ok(path);
    }
    let mut candidates = vec![];
    if cfg!(target_os = "macos") {
        candidates.push(PathBuf::from(
            "/Applications/Google Chrome.app/Contents/MacOS/Google Chrome",
        ));
        if let Some(home) = std::env::var_os("HOME") {
            candidates.push(
                PathBuf::from(home)
                    .join("Applications/Google Chrome.app/Contents/MacOS/Google Chrome"),
            );
        }
    } else if cfg!(target_os = "windows") {
        for name in ["PROGRAMFILES", "PROGRAMFILES(X86)", "LOCALAPPDATA"] {
            if let Some(root) = std::env::var_os(name) {
                candidates.push(PathBuf::from(root).join("Google/Chrome/Application/chrome.exe"));
            }
        }
    }
    candidates
        .into_iter()
        .find(|path| path.is_file())
        .context("未找到 Google Chrome，请安装 Chrome 或在设置中选择程序路径")
}

#[derive(serde::Serialize)]
pub struct BrowserProfile {
    pub data_dir: String,
    pub profile: String,
    pub name: String,
    pub email: String,
}

/// Only read Chrome's profile index and check directory structure; never read cookies.
pub fn discover_profiles() -> Vec<BrowserProfile> {
    let mut roots = Vec::new();
    #[cfg(target_os = "macos")]
    if let Some(home) = std::env::var_os("HOME") {
        roots.push(PathBuf::from(home).join("Library/Application Support/Google/Chrome"));
    }
    #[cfg(target_os = "windows")]
    if let Some(local) = std::env::var_os("LOCALAPPDATA") {
        roots.push(PathBuf::from(local).join("Google/Chrome/User Data"));
    }
    roots
        .into_iter()
        .flat_map(|root| profiles_in(&root))
        .collect()
}

fn profiles_in(root: &Path) -> Vec<BrowserProfile> {
    let index = std::fs::read(root.join("Local State"))
        .ok()
        .and_then(|bytes| serde_json::from_slice::<Value>(&bytes).ok());
    let Some(cache) = index
        .as_ref()
        .and_then(|v| v["profile"]["info_cache"].as_object())
    else {
        return Vec::new();
    };
    let mut profiles = Vec::new();
    for (profile, info) in cache {
        let Ok(root) = validate_profile(&root.to_string_lossy(), profile) else {
            continue;
        };
        profiles.push(BrowserProfile {
            data_dir: root.to_string_lossy().into_owned(),
            profile: profile.clone(),
            name: info["name"]
                .as_str()
                .filter(|s| !s.is_empty())
                .unwrap_or(profile)
                .into(),
            email: info["user_name"].as_str().unwrap_or_default().into(),
        });
    }
    profiles.sort_by(|a, b| a.name.cmp(&b.name).then(a.profile.cmp(&b.profile)));
    profiles
}

pub fn validate_profile(data_dir: &str, profile_name: &str) -> Result<PathBuf> {
    let root = Path::new(data_dir);
    if !root.is_absolute() || !root.is_dir() || !root.join("Local State").is_file() {
        bail!(
            "请选择包含 Local State 的浏览器 User Data 根目录，不能选择 Default / Profile 子目录"
        );
    }
    if profile_name.is_empty()
        || profile_name == "."
        || profile_name == ".."
        || profile_name.contains(['/', '\\'])
        || profile_name.starts_with('-')
    {
        bail!("请输入资料子目录名，例如 Default 或 Profile 1");
    }
    let root = root.canonicalize()?;
    let child = root
        .join(profile_name)
        .canonicalize()
        .context("资料子目录不存在，请核对 Default / Profile 名称")?;
    if child.parent() != Some(root.as_path()) || !child.join("Preferences").is_file() {
        bail!("资料子目录必须是该 User Data 下含 Preferences 的直接子目录");
    }
    Ok(root)
}

impl BrowserPool {
    pub fn new(root: PathBuf) -> Self {
        Self {
            root,
            sessions: Mutex::new(HashMap::new()),
        }
    }
    pub async fn session(
        &self,
        id: &str,
        configured: &str,
        proxy: &str,
        data_dir: &str,
        profile_name: &str,
    ) -> Result<Arc<Mutex<Session>>> {
        // IDs are also profile directory names: reject traversal before touching disk.
        uuid::Uuid::parse_str(id).context("账号 ID 无效")?;
        let existing = self.sessions.lock().await.get(id).cloned();
        if let Some(session) = existing {
            if session.lock().await.reusable().await {
                return Ok(session.clone());
            }
        }
        let mut sessions = self.sessions.lock().await;
        // A concurrent first login may have launched this account while we waited.
        if let Some(session) = sessions.get(id) {
            if session.lock().await.reusable().await {
                return Ok(session.clone());
            }
        }
        let existing = !data_dir.is_empty();
        let profile = if existing {
            validate_profile(data_dir, profile_name)?
        } else {
            self.root.join(id)
        };
        let mut launched = Session::launch_profile(
            &executable(configured)?,
            &profile,
            proxy,
            profile_name,
            existing,
        )
        .await?;
        launched
            .navigate("https://flow.google.com/projects")
            .await?;
        let session = Arc::new(Mutex::new(launched));
        sessions.insert(id.into(), session.clone());
        Ok(session)
    }
    pub async fn close_all(&self) {
        let sessions = std::mem::take(&mut *self.sessions.lock().await);
        for session in sessions.into_values() {
            session.lock().await.close().await;
        }
    }
}

impl Session {
    #[cfg(test)]
    async fn launch(executable: &Path, profile: &Path, proxy: &str) -> Result<Self> {
        Self::launch_profile(executable, profile, proxy, "", false).await
    }
    async fn launch_profile(
        executable: &Path,
        profile: &Path,
        proxy: &str,
        profile_name: &str,
        existing: bool,
    ) -> Result<Self> {
        if existing && profile.join("SingletonLock").symlink_metadata().is_ok() {
            bail!(
                "已有资料目录正被浏览器使用或遗留锁存在，请先正常退出该浏览器；应用不会删除锁文件"
            );
        }
        tokio::fs::create_dir_all(profile).await?;
        let active_port = profile.join("DevToolsActivePort");
        if !existing {
            let _ = tokio::fs::remove_file(&active_port).await;
        }
        let launched_at = SystemTime::now();
        let mut command = Command::new(executable);
        command
            .arg(format!("--user-data-dir={}", profile.display()))
            .args([
                "--remote-debugging-port=0",
                "--remote-debugging-address=127.0.0.1",
                "--no-first-run",
                "--no-default-browser-check",
                "about:blank",
            ])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .kill_on_drop(true);
        if !profile_name.is_empty() {
            command.arg(format!("--profile-directory={profile_name}"));
        }
        if !proxy.trim().is_empty() {
            let url = url::Url::parse(proxy)?;
            if !url.username().is_empty() || url.password().is_some() {
                bail!("Chrome 代理暂不支持 URL 内用户名密码，请使用本机代理转发");
            }
            command.arg(format!("--proxy-server={proxy}"));
        }
        #[cfg(windows)]
        command.creation_flags(0x08000000);
        let mut child = command.spawn().context("Chrome 启动失败")?;
        let client = reqwest::Client::builder()
            .no_proxy()
            .timeout(Duration::from_secs(3))
            .build()?;
        let connect = async {
            for _ in 0..100 {
                if child.try_wait()?.is_some() {
                    bail!("Chrome 已退出，可能该应用的浏览器资料正被另一实例使用");
                }
                if existing
                    && !tokio::fs::metadata(&active_port)
                        .await
                        .and_then(|m| m.modified())
                        .is_ok_and(|t| t >= launched_at)
                {
                    sleep(Duration::from_millis(200)).await;
                    continue;
                }
                if let Ok(text) = tokio::fs::read_to_string(&active_port).await {
                    if let Some(port) = text.lines().next().and_then(|v| v.parse::<u16>().ok()) {
                        let targets: Value = client
                            .get(format!("http://127.0.0.1:{port}/json/list"))
                            .send()
                            .await?
                            .error_for_status()?
                            .json()
                            .await?;
                        if let Some(target) = targets
                            .as_array()
                            .and_then(|items| items.iter().find(|t| t["type"] == "page"))
                        {
                            let socket_url = target["webSocketDebuggerUrl"]
                                .as_str()
                                .context("Chrome 缺少调试连接")?;
                            let (socket, _) = connect_async(socket_url)
                                .await
                                .context("连接 Chrome 失败")?;
                            return Ok(socket);
                        }
                    }
                }
                sleep(Duration::from_millis(200)).await;
            }
            bail!("Chrome 调试接口启动超时")
        };
        let socket = match timeout(Duration::from_secs(30), connect).await {
            Ok(Ok(socket)) => socket,
            _ => {
                let _ = child.kill().await;
                bail!("无法启动浏览器调试会话；请退出占用该资料目录的浏览器。新版 Chrome 默认日常目录可能禁止调试，请改用独立登录目录");
            }
        };
        let mut session = Self {
            child,
            socket,
            sequence: 0,
        };
        session.call("Page.enable", json!({})).await?;
        session.call("Runtime.enable", json!({})).await?;
        session.call("Network.enable", json!({})).await?;
        session
            .call(
                "Page.addScriptToEvaluateOnNewDocument",
                json!({"source":include_str!("../assets/recaptcha-hook.js")}),
            )
            .await?;
        Ok(session)
    }
    async fn reusable(&mut self) -> bool {
        if !matches!(self.child.try_wait(), Ok(None)) {
            return false;
        }
        // A Chrome process can outlive the page or its debugging socket. Probe before
        // admitting a new operation; never retry a generation whose result is unknown.
        if matches!(
            timeout(Duration::from_secs(4), self.evaluate("true")).await,
            Ok(Ok(Value::Bool(true)))
        ) {
            return true;
        }
        self.close().await;
        false
    }
    pub async fn call(&mut self, method: &str, params: Value) -> Result<Value> {
        self.sequence += 1;
        let id = self.sequence;
        self.socket
            .send(Message::Text(
                json!({"id":id,"method":method,"params":params})
                    .to_string()
                    .into(),
            ))
            .await
            .context("Chrome 连接已断开")?;
        timeout(Duration::from_secs(180), async {
            while let Some(message) = self.socket.next().await {
                let message = message.context("Chrome 连接读取失败")?;
                if let Message::Text(text) = message {
                    let value: Value = serde_json::from_str(&text)?;
                    if value["id"].as_u64() == Some(id) {
                        if value.get("error").is_some() {
                            bail!("Chrome 命令 {method} 执行失败");
                        }
                        return Ok(value["result"].clone());
                    }
                }
            }
            bail!("Chrome 连接已关闭")
        })
        .await
        .context("Chrome 操作超时；请求可能已被接受，请先核对结果")?
    }
    pub async fn evaluate(&mut self, expression: &str) -> Result<Value> {
        let result = self
            .call(
                "Runtime.evaluate",
                json!({"expression":expression,"awaitPromise":true,"returnByValue":true}),
            )
            .await?;
        if result.get("exceptionDetails").is_some() {
            bail!("浏览器脚本执行失败，请检查 Flow 页面登录状态");
        }
        Ok(result["result"]["value"].clone())
    }
    pub async fn navigate(&mut self, url: &str) -> Result<()> {
        let result = self.call("Page.navigate", json!({"url":url})).await?;
        if result.get("errorText").is_some() {
            bail!("Flow 页面导航失败，请检查网络或代理");
        }
        for _ in 0..60 {
            sleep(Duration::from_millis(250)).await;
            if self
                .evaluate("document.readyState === 'complete'")
                .await
                .unwrap_or(Value::Bool(false))
                == true
            {
                return Ok(());
            }
        }
        bail!("Flow 页面加载超时")
    }
    pub async fn verify_account(&mut self, email: &str) -> Result<()> {
        let expression = format!("[...document.querySelectorAll('[aria-label]')].some(el=>(el.getAttribute('aria-label')||'').toLowerCase().includes({}))", serde_json::to_string(&format!("({})", email.to_lowercase()))?);
        if self.evaluate(&expression).await? != true {
            bail!("浏览器登录账号与所选邮箱不一致或尚未登录，请在专用 Chrome 中确认账号后同步");
        }
        Ok(())
    }
    pub async fn project(&mut self, id: &str, email: &str) -> Result<()> {
        uuid::Uuid::parse_str(id).context("Flow 项目 ID 格式错误")?;
        let target = format!("https://flow.google.com/project/{id}");
        if self.evaluate("location.href").await?.as_str() != Some(target.as_str()) {
            self.navigate(&target).await?;
        }
        self.verify_account(email).await
    }
    pub async fn captcha(&mut self, action: &str) -> Result<String> {
        for _ in 0..80 {
            if self.evaluate("!!window.grecaptcha?.enterprise").await? == true {
                break;
            }
            sleep(Duration::from_millis(250)).await;
        }
        let script = include_str!("../assets/recaptcha-execute.js")
            .replace("__SITE_KEY__", "'6LdsFiUsAAAAAIjVDZcuLhaHiDn5nnHVXVRQGeMV'")
            .replace("__REQUESTED_ACTION__", &serde_json::to_string(action)?)
            .replace("__TIMEOUT_MS__", "45000");
        let result = self.evaluate(&script).await?;
        result["token"]
            .as_str()
            .filter(|v| !v.is_empty())
            .map(str::to_owned)
            .context("未获取到验证码，请检查 Chrome 中的 Flow 登录与网络")
    }
    pub async fn fetch(
        &mut self,
        url: &str,
        method: &str,
        body: Option<&str>,
        content_type: &str,
    ) -> Result<Value> {
        let input = json!({"url":url,"method":method,"body":body,"content_type":content_type});
        self.evaluate(&format!(r#"(async()=>{{const i={input};const r=await fetch(i.url,{{method:i.method,credentials:'include',headers:{{'Content-Type':i.content_type,'x-same-domain':'1'}},...(i.body===null?{{}}:{{body:i.body}}),signal:AbortSignal.timeout(90000)}});return {{status:r.status,text:await r.text()}}}})()"#)).await
    }
    pub async fn close(&mut self) {
        let _ = timeout(
            Duration::from_secs(2),
            self.call("Browser.close", json!({})),
        )
        .await;
        if timeout(Duration::from_secs(5), self.child.wait())
            .await
            .is_err()
        {
            let _ = self.child.kill().await;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn profile_discovery_lists_named_profiles_and_skips_invalid_entries() -> Result<()> {
        let dir = tempfile::tempdir()?;
        std::fs::create_dir(dir.path().join("Profile 1"))?;
        std::fs::write(dir.path().join("Profile 1/Preferences"), "{}")?;
        std::fs::write(
            dir.path().join("Local State"),
            serde_json::to_vec(&json!({
                "profile": {"info_cache": {
                    "Profile 1": {"name":"Work", "user_name":"user@example.test"},
                    "Missing": {"name":"Deleted"},
                    "../escape": {"name":"Invalid"}
                }}
            }))?,
        )?;
        let profiles = profiles_in(dir.path());
        assert_eq!(profiles.len(), 1);
        assert_eq!(profiles[0].name, "Work");
        assert_eq!(profiles[0].email, "user@example.test");
        assert_eq!(profiles[0].profile, "Profile 1");
        std::fs::write(dir.path().join("Local State"), "invalid")?;
        assert!(profiles_in(dir.path()).is_empty());
        Ok(())
    }
    #[test]
    fn existing_profile_validation_rejects_subdirectory_and_traversal() -> Result<()> {
        let dir = tempfile::tempdir()?;
        std::fs::write(dir.path().join("Local State"), "{}")?;
        std::fs::create_dir(dir.path().join("Profile 1"))?;
        std::fs::write(dir.path().join("Profile 1/Preferences"), "{}")?;
        assert!(validate_profile(dir.path().to_str().unwrap(), "Profile 1").is_ok());
        assert!(validate_profile(dir.path().to_str().unwrap(), "../Profile 1").is_err());
        assert!(validate_profile(dir.path().to_str().unwrap(), "").is_err());
        assert!(
            validate_profile(dir.path().join("Profile 1").to_str().unwrap(), "Default").is_err()
        );
        Ok(())
    }
    #[tokio::test]
    #[ignore = "launches isolated browser profiles; requires installed Chrome or FLOW2API_TEST_BROWSER"]
    async fn existing_profile_reopens_without_removing_data_or_taking_over_running_browser(
    ) -> Result<()> {
        let directory = tempfile::tempdir()?;
        let executable = executable(&std::env::var("FLOW2API_TEST_BROWSER").unwrap_or_default())?;
        let profile = directory.path().join("已有资料 with spaces");
        let mut first = Session::launch(&executable, &profile, "").await?;
        first
            .evaluate("localStorage.setItem('unused','test')")
            .await
            .ok();
        first.close().await;
        std::fs::write(profile.join("preserve.txt"), "preserved")?;
        validate_profile(profile.to_str().unwrap(), "Default")?;
        let mut second =
            Session::launch_profile(&executable, &profile, "", "Default", true).await?;
        assert!(second.reusable().await);
        #[cfg(unix)]
        {
            assert!(
                Session::launch_profile(&executable, &profile, "", "Default", true)
                    .await
                    .is_err()
            );
            assert!(second.reusable().await);
        }
        second.close().await;
        assert_eq!(
            std::fs::read_to_string(profile.join("preserve.txt"))?,
            "preserved"
        );
        Ok(())
    }
    #[tokio::test]
    #[ignore = "launches a dedicated local Chrome; requires Google Chrome"]
    async fn browser_transport_detects_closed_page_and_releases_profile() -> Result<()> {
        let directory = tempfile::tempdir()?;
        let executable = executable("")?;
        let profile = directory.path().join("专用 Chrome profile");
        let mut session = Session::launch(&executable, &profile, "").await?;
        assert!(session.reusable().await);
        let app = crate::App::open(&directory.path().join("app-data"))?;
        let mut account = crate::model::Account {
            id: uuid::Uuid::new_v4().to_string(),
            email: "not-logged-in@example.test".into(),
            enabled: true,
            project_id: String::new(),
            last_synced: Some(crate::model::now()),
            credits: None,
            browser_data_dir: String::new(),
            browser_profile: String::new(),
        };
        app.store.save_account(&account)?;
        app.refresh()?;
        assert_eq!(app.status().ready_accounts, 1);
        assert!(app.verify_login(&mut session, &mut account).await.is_err());
        assert_eq!(app.status().ready_accounts, 0);
        assert!(app.store.account(&account.id)?.last_synced.is_none());
        assert!(app.status().browser.contains("需要重新登录同步"));
        session.socket.close(None).await?;
        assert!(!session.reusable().await);
        assert!(session.child.try_wait()?.is_some());
        let mut reopened = Session::launch(&executable, &profile, "").await?;
        assert!(reopened.reusable().await);
        reopened.close().await;
        assert!(reopened.child.try_wait()?.is_some());
        Ok(())
    }
}
