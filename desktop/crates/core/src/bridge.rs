//! A browser connection carries commands, never Google cookies. Commands are delivered
//! once: an uncertain generation must never be silently submitted a second time.
use crate::{browser::Session, flow, model::*, App};
use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};
use tokio::sync::{mpsc, oneshot};

const ONLINE: Duration = Duration::from_secs(15);
#[derive(Serialize)]
pub struct ConnectionStatus {
    pub account_id: String,
    pub connected: bool,
    pub state: String,
    pub message: String,
}
#[derive(Serialize, Clone)]
pub struct Command {
    pub id: String,
    pub operation: String,
    pub input: Value,
}
#[derive(Deserialize)]
pub struct Completion {
    pub id: String,
    pub value: Option<Value>,
    pub error: Option<String>,
}
pub struct Peer {
    pub validating: std::sync::atomic::AtomicBool,
    pub account_id: String,
    sender: mpsc::Sender<Command>,
    receiver: tokio::sync::Mutex<mpsc::Receiver<Command>>,
    pending: Mutex<HashMap<String, oneshot::Sender<Result<Value>>>>,
    last_seen: Mutex<Instant>,
    state: Mutex<(String, String)>,
    /// Serializes whole workflows, including identity checks, RPCs and captcha.
    pub session: tokio::sync::Mutex<()>,
}
impl Peer {
    fn new(account_id: String) -> Arc<Self> {
        let (sender, receiver) = mpsc::channel(8);
        Arc::new(Self {
            validating: std::sync::atomic::AtomicBool::new(false),
            account_id,
            sender,
            receiver: tokio::sync::Mutex::new(receiver),
            pending: Mutex::new(HashMap::new()),
            last_seen: Mutex::new(Instant::now()),
            state: Mutex::new(("validating".into(), "正在校验 Flow 身份与余额".into())),
            session: tokio::sync::Mutex::new(()),
        })
    }
    pub fn online(&self) -> bool {
        self.last_seen.lock().unwrap().elapsed() < ONLINE
    }
    pub fn touch(&self) {
        *self.last_seen.lock().unwrap() = Instant::now();
    }
    pub fn revalidate(&self) {
        let mut state = self.state.lock().unwrap();
        if state.0 != "validating" {
            *state = ("validating".into(), "正在重新校验 Flow 身份与余额".into());
            self.validating
                .store(false, std::sync::atomic::Ordering::SeqCst);
        }
    }
    pub fn status(&self) -> ConnectionStatus {
        let (state, message) = self.state.lock().unwrap().clone();
        ConnectionStatus {
            account_id: self.account_id.clone(),
            connected: self.online(),
            state,
            message,
        }
    }
    pub fn set_state(&self, state: &str, message: &str) {
        *self.state.lock().unwrap() = (state.into(), message.into());
    }
    pub async fn request(&self, operation: &str, input: Value) -> Result<Value> {
        if !self.online() {
            bail!("Flow 标签页已断开，请在默认浏览器中打开 Flow 并重新连接");
        }
        let id = uuid::Uuid::new_v4().to_string();
        let (sender, receiver) = oneshot::channel();
        self.pending.lock().unwrap().insert(id.clone(), sender);
        if self
            .sender
            .try_send(Command {
                id: id.clone(),
                operation: operation.into(),
                input,
            })
            .is_err()
        {
            self.pending.lock().unwrap().remove(&id);
            bail!("浏览器连接繁忙，请稍后重试");
        }
        let result = tokio::time::timeout(Duration::from_secs(110), receiver).await;
        self.pending.lock().unwrap().remove(&id);
        match result {
            Ok(Ok(result)) => result,
            _ => bail!("Flow 标签页响应超时；请求可能已提交，请核对 Flow 结果后再操作"),
        }
    }
    pub async fn poll(&self, result: Option<Completion>) -> Vec<Command> {
        self.touch();
        if let Some(result) = result {
            if let Some(sender) = self.pending.lock().unwrap().remove(&result.id) {
                let value = match result.error {
                    Some(_) => Err(anyhow::anyhow!(
                        "Flow 浏览器操作失败，请检查页面登录、网络或重新连接"
                    )),
                    None => Ok(result.value.unwrap_or(Value::Null)),
                };
                let _ = sender.send(value);
            }
        }
        let mut receiver = self.receiver.lock().await;
        while let Ok(command) = receiver.try_recv() {
            if self.pending.lock().unwrap().contains_key(&command.id) {
                return vec![command];
            }
        }
        vec![]
    }
    fn disconnect(&self) {
        for (_, sender) in self.pending.lock().unwrap().drain() {
            let _ = sender.send(Err(anyhow::anyhow!(
                "浏览器连接已替换或断开；请先核对 Flow 结果"
            )));
        }
        *self.last_seen.lock().unwrap() = Instant::now() - ONLINE;
    }
}
#[derive(Default)]
pub struct Hub {
    pairing: Mutex<Option<(String, Instant)>>,
    peers: Mutex<HashMap<String, Arc<Peer>>>,
}
impl Hub {
    pub fn pairing_code(&self) -> String {
        let code = uuid::Uuid::new_v4().simple().to_string()[..12].to_uppercase();
        *self.pairing.lock().unwrap() = Some((code.clone(), Instant::now()));
        code
    }
    fn consume_code(&self, code: &str) -> Result<()> {
        let mut pairing = self.pairing.lock().unwrap();
        if !pairing.as_ref().is_some_and(|(value, issued)| {
            value == &code.trim().to_uppercase() && issued.elapsed() < Duration::from_secs(300)
        }) {
            bail!("连接码无效或已过期，请回应用重新获取（有效期 5 分钟）");
        }
        pairing.take();
        Ok(())
    }
    pub fn get(&self, id: &str) -> Option<Arc<Peer>> {
        self.peers.lock().unwrap().get(id).cloned()
    }
    pub fn online(&self, id: &str) -> bool {
        self.get(id).is_some_and(|p| p.online())
    }
    pub fn ready(&self, id: &str) -> bool {
        self.get(id)
            .is_some_and(|p| p.online() && p.state.lock().unwrap().0 == "ready")
    }
    pub fn statuses(&self) -> Vec<ConnectionStatus> {
        self.peers
            .lock()
            .unwrap()
            .values()
            .map(|p| p.status())
            .collect()
    }
    fn connect(&self, id: &str) -> Arc<Peer> {
        let mut peers = self.peers.lock().unwrap();
        if let Some(previous) = peers.remove(id) {
            previous.disconnect();
        }
        let peer = Peer::new(id.into());
        peers.insert(id.into(), peer.clone());
        peer
    }
    fn resume(&self, id: &str) -> Arc<Peer> {
        let mut peers = self.peers.lock().unwrap();
        peers
            .entry(id.into())
            .or_insert_with(|| Peer::new(id.into()))
            .clone()
    }
    pub fn remove(&self, id: &str) {
        if let Some(peer) = self.peers.lock().unwrap().remove(id) {
            peer.disconnect();
        }
    }
    pub fn clear(&self) {
        for peer in self.peers.lock().unwrap().drain().map(|(_, p)| p) {
            peer.disconnect();
        }
    }
}
fn digest(token: &str) -> String {
    format!("{:x}", Sha256::digest(token.as_bytes()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{
        body::{to_bytes, Body},
        http::{Request, StatusCode},
    };
    use tower::ServiceExt;

    async fn browser_round(app: &Arc<App>, token: &str, result: Option<Value>) -> Result<Value> {
        let response = crate::api::router(app.clone())
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/browser-bridge/poll")
                    .header("Content-Type", "application/json")
                    .header("Authorization", format!("Bearer {token}"))
                    .body(Body::from(json!({"result":result}).to_string()))?,
            )
            .await?;
        assert_eq!(response.status(), StatusCode::OK);
        Ok(serde_json::from_slice(
            &to_bytes(response.into_body(), 1024 * 1024).await?,
        )?)
    }
    async fn validate_fixture(
        app: &Arc<App>,
        token: &str,
        email: &str,
        credits: Value,
    ) -> Result<Value> {
        let mut result = None;
        for _ in 0..300 {
            let response = browser_round(app, token, result.take()).await?;
            if response["connection"]["state"] != "validating" {
                return Ok(response);
            }
            if let Some(command) = response["commands"].as_array().and_then(|a| a.first()) {
                let value = match command["operation"].as_str().unwrap() {
                    "identity" => json!({"email":email}),
                    "fetch" if command["input"]["method"] == "GET" => {
                        json!({"status":200,"text":r#"boq_labs-ai-sandbox-frontend_test "FdrFJe":"123" "SNlM0e":"test""#})
                    }
                    "fetch" => {
                        json!({"status":200,"text":json!([["wrb.fr","nzlxg",credits.to_string()]]).to_string()})
                    }
                    other => bail!("unexpected operation {other}"),
                };
                result = Some(json!({"id":command["id"],"value":value}));
            }
            tokio::time::sleep(Duration::from_millis(1)).await;
        }
        bail!("validation did not finish")
    }
    #[tokio::test]
    async fn connection_verifies_real_rpc_path_and_reuses_account_id() -> Result<()> {
        let dir = tempfile::tempdir()?;
        let app = App::open(dir.path())?;
        let code = app.bridge.pairing_code();
        assert!(app
            .pair_browser("wrong", "user@example.test")
            .await
            .is_err());
        let (id, token) = app.pair_browser(&code, "user@example.test").await?;
        assert!(app.pair_browser(&code, "user@example.test").await.is_err());
        assert_eq!(app.status().ready_accounts, 0);
        let state = validate_fixture(&app, &token, "user@example.test", json!([0])).await?;
        assert_eq!(state["connection"]["state"], "ready");
        let account = app.store.account(&id)?;
        assert_eq!(account.credits, Some(0));
        assert!(account.last_synced.is_some());
        assert!(account.browser_bridge);
        assert!(account.browser_data_dir.is_empty());
        assert_eq!(app.status().ready_accounts, 1);
        // Reconnect with no user-entered email directory decision: same identity -> same ID.
        let code = app.bridge.pairing_code();
        let (again, replacement) = app.pair_browser(&code, "USER@example.test").await?;
        assert_eq!(again, id);
        assert!(app.browser_peer(&token).is_err());
        assert_eq!(app.status().ready_accounts, 0);
        validate_fixture(&app, &replacement, "user@example.test", json!([52])).await?;
        assert_eq!(app.store.accounts()?.len(), 1);
        assert_eq!(app.store.account(&id)?.credits, Some(52));
        // Reload uses persisted hashed connection, but cached sync is never enough.
        app.bridge.clear();
        assert_eq!(app.status().ready_accounts, 0);
        let peer = app.browser_peer(&replacement)?;
        assert_eq!(app.status().ready_accounts, 0);
        validate_fixture(&app, &replacement, "user@example.test", json!([53])).await?;
        assert_eq!(app.status().ready_accounts, 1);
        *peer.last_seen.lock().unwrap() = Instant::now() - ONLINE;
        assert_eq!(app.status().ready_accounts, 0);
        Ok(())
    }
    #[tokio::test]
    async fn mismatch_and_invalid_credit_payload_never_report_ready() -> Result<()> {
        for (actual, credits) in [
            ("other@example.test", json!([12])),
            ("user@example.test", json!([null])),
            ("user@example.test", json!([-1])),
        ] {
            let dir = tempfile::tempdir()?;
            let app = App::open(dir.path())?;
            let (id, token) = app
                .pair_browser(&app.bridge.pairing_code(), "user@example.test")
                .await?;
            let response = validate_fixture(&app, &token, actual, credits).await?;
            assert_eq!(response["connection"]["state"], "failed");
            assert!(app.store.account(&id)?.last_synced.is_none());
            assert_eq!(app.status().ready_accounts, 0);
        }
        Ok(())
    }
    #[tokio::test]
    async fn command_delivery_is_once_and_replacement_cancels_waiters() -> Result<()> {
        let peer = Peer::new(uuid::Uuid::new_v4().to_string());
        let p = peer.clone();
        let waiter = tokio::spawn(async move { p.request("fetch", Value::Null).await });
        tokio::task::yield_now().await;
        let commands = peer.poll(None).await;
        assert_eq!(commands.len(), 1);
        assert!(peer.poll(None).await.is_empty());
        peer.disconnect();
        assert!(waiter.await?.is_err());
        assert!(!peer.online());
        Ok(())
    }
}
impl App {
    pub async fn account_session(
        &self,
        account: &Account,
    ) -> Result<Arc<tokio::sync::Mutex<Session>>> {
        if account.browser_bridge {
            let peer = self
                .bridge
                .get(&account.id)
                .filter(|p| p.online())
                .context("Flow 标签页未连接；请在默认浏览器打开 Flow，连接后再同步")?;
            return Ok(self.browsers.bridge_session(&account.id, peer).await);
        }
        let settings = self.store.settings()?;
        self.browsers
            .session(
                &account.id,
                &settings.browser_executable,
                &settings.proxy_url,
                &account.browser_data_dir,
                &account.browser_profile,
            )
            .await
    }
    pub async fn pair_browser(
        self: &Arc<Self>,
        code: &str,
        email: &str,
    ) -> Result<(String, String)> {
        let _gate = self.job_gate.lock().await;
        self.ensure_available()?;
        if self.status().busy() {
            bail!("请等待当前任务完成后连接浏览器");
        }
        let email = email.trim().to_lowercase();
        if !email.contains('@') || email.len() > 254 {
            bail!("Flow 页面尚未登录，请先登录后连接");
        }
        self.bridge.consume_code(code)?;
        let mut account = self
            .store
            .accounts()?
            .into_iter()
            .find(|a| a.email.eq_ignore_ascii_case(&email))
            .unwrap_or_else(|| Account {
                id: uuid::Uuid::new_v4().to_string(),
                email,
                enabled: true,
                project_id: String::new(),
                last_synced: None,
                credits: None,
                browser_data_dir: String::new(),
                browser_profile: String::new(),
                browser_bridge: true,
            });
        account.browser_bridge = true;
        account.last_synced = None;
        let token = format!(
            "{}{}",
            uuid::Uuid::new_v4().simple(),
            uuid::Uuid::new_v4().simple()
        );
        self.store
            .save_browser_connection(&account, &digest(&token))?;
        self.bridge.connect(&account.id);
        self.refresh()?;
        let id = account.id.clone();
        Ok((id, token))
    }
    /// A valid persisted connection may resume after an application restart. Revalidate
    /// it through the real page before allowing any generation.
    pub fn browser_peer(self: &Arc<Self>, token: &str) -> Result<Arc<Peer>> {
        let id = self
            .store
            .browser_connection(&digest(token))?
            .context("浏览器连接已失效，请重新获取连接码")?;
        Ok(self.bridge.resume(&id))
    }
    pub async fn validate_browser(self: &Arc<Self>, peer: Arc<Peer>) {
        let _session = peer.session.lock().await;
        let outcome = async {
            let mut account = self.store.account(&peer.account_id)?;
            let expected_email = account.email.clone();
            account.last_synced = None;
            self.store.save_account(&account)?;
            let session = self
                .browsers
                .bridge_session(&account.id, peer.clone())
                .await;
            let mut session = session.lock().await;
            session.verify_account(&account.email).await?;
            let response = flow::rpc(&mut session, "nzlxg", json!([]), None).await?;
            let credits = flow::credits(&response)?;
            // A reconnect or account deletion while a request was in flight must not
            // resurrect the old account or publish its stale validation result.
            let current = self.bridge.get(&account.id).context("连接已关闭")?;
            if !Arc::ptr_eq(&current, &peer) {
                bail!("连接已被替换");
            }
            account = self.store.account(&account.id)?;
            if !account.browser_bridge || account.email != expected_email {
                bail!("账号连接配置已改变，请重新连接");
            }
            account.credits = Some(credits);
            account.last_synced = Some(now());
            self.store.save_account(&account)?;
            self.refresh()?;
            Ok::<_, anyhow::Error>(())
        }
        .await;
        match outcome {
            Ok(()) => peer.set_state("ready", "身份与余额校验通过，可以创作"),
            Err(error) => {
                peer.set_state("failed", &error.to_string());
                self.change_status(|s| s.last_error = Some(error.to_string()));
            }
        }
        let _ = self.refresh();
    }
}
