//! Flow Music (`flowmusic.app`) browser bridge and song generation workflow.
//!
//! Credentials and Supabase tokens stay inside the user's browser tab. The desktop app
//! only sends allowlisted `/__api` commands over the local music bridge and downloads
//! finished `.m4a` clips from public storage.
use crate::{
    bridge::{digest, Peer},
    generation::http_client,
    model::*,
    App,
};
use anyhow::{bail, Context, Result};
use futures_util::StreamExt;
use serde_json::{json, Value};
use std::{sync::Arc, time::Duration};
use tokio::time::sleep;

const CLIP_BASE_URL: &str = "https://storage.googleapis.com/producer-app-public/clips";

pub fn build_message(
    prompt: &str,
    duration_seconds: Option<u32>,
    instrumental: bool,
    lyrics: Option<&str>,
) -> String {
    let mut rules = vec![
        "Create exactly ONE song right now with your song tool. Do not ask questions first."
            .to_string(),
    ];
    if let Some(seconds) = duration_seconds.filter(|s| *s >= 10) {
        rules.push(format!("Target length: about {seconds} seconds."));
    }
    if instrumental {
        rules.push("Instrumental only: no vocals, no lyrics.".into());
    }
    let mut text = prompt.trim().to_string();
    if !instrumental {
        if let Some(lyrics) = lyrics.map(str::trim).filter(|s| !s.is_empty()) {
            text.push_str("\n\nUse exactly these lyrics, word for word:\n");
            text.push_str(lyrics);
        }
    }
    format!("{text}\n\n{}", rules.join(" "))
}

async fn music_api(peer: &Peer, method: &str, path: &str, body: Option<Value>) -> Result<Value> {
    let response = peer
        .request(
            "music_api",
            json!({
                "method": method,
                "path": path,
                "body": body,
            }),
        )
        .await?;
    let status = response["status"].as_u64().unwrap_or(0);
    if !(200..300).contains(&status) {
        let snippet = response["text"]
            .as_str()
            .unwrap_or_default()
            .chars()
            .take(240)
            .collect::<String>();
        if status == 401 || status == 403 {
            bail!("Flow Music 登录已过期或无权限（HTTP {status}），请在浏览器刷新 flowmusic.app 后重试");
        }
        if status == 429 {
            bail!("Flow Music 请求过于频繁（HTTP 429），请稍后再试");
        }
        bail!("Flow Music 接口错误（HTTP {status} {method} {path}）：{snippet}");
    }
    if let Some(data) = response.get("json").filter(|v| !v.is_null()) {
        return Ok(data.clone());
    }
    let text = response["text"].as_str().unwrap_or_default().trim();
    if text.is_empty() {
        return Ok(Value::Null);
    }
    serde_json::from_str(text).context("Flow Music 返回数据格式无效")
}

fn parse_credits(value: &Value) -> Result<i64> {
    value["data"]["credits_remaining"]
        .as_i64()
        .filter(|v| *v >= 0)
        .context("Flow Music 未返回有效积分余额，请确认页面已登录")
}

fn parse_plan(value: &Value) -> String {
    value["data"]["subscription_tier"]
        .as_str()
        .or_else(|| value["data"]["subscription_plan"].as_str())
        .unwrap_or_default()
        .trim()
        .to_string()
}

impl App {
    pub async fn pair_music_browser(self: &Arc<Self>, email: &str) -> Result<(String, String)> {
        let _gate = self.job_gate.lock().await;
        self.ensure_available()?;
        let email = email.trim().to_lowercase();
        if !email.contains('@') || email.len() > 254 {
            bail!("Flow Music 页面尚未登录，请先登录 Google 账号后连接");
        }
        let mut account = self
            .store
            .accounts()?
            .into_iter()
            .find(|a| a.email.eq_ignore_ascii_case(&email))
            .unwrap_or_else(|| Account {
                id: uuid::Uuid::new_v4().to_string(),
                email,
                enabled: true,
                browser_bridge: true,
                ..Account::default()
            });
        account.browser_bridge = true;
        account.browser_data_dir.clear();
        account.browser_profile.clear();
        account.music_last_synced = None;
        let token = format!(
            "{}{}",
            uuid::Uuid::new_v4().simple(),
            uuid::Uuid::new_v4().simple()
        );
        self.store
            .save_music_connection(&account, &digest(&token))?;
        self.music_bridge.connect(&account.id);
        self.refresh()?;
        Ok((account.id, token))
    }

    pub fn music_peer(self: &Arc<Self>, token: &str) -> Result<Arc<Peer>> {
        let id = self
            .store
            .music_connection(&digest(token))?
            .context("Flow Music 浏览器连接已失效，请重新连接")?;
        Ok(self.music_bridge.resume(&id))
    }

    pub async fn validate_music_browser(self: &Arc<Self>, peer: Arc<Peer>) {
        let _session = peer.session.lock().await;
        let outcome = async {
            let mut account = self.store.account(&peer.account_id)?;
            let expected_email = account.email.clone();
            account.music_last_synced = None;
            self.store.save_account(&account)?;
            let identity = peer.request("identity", Value::Null).await?;
            let actual = identity["email"]
                .as_str()
                .map(|v| v.trim().to_lowercase())
                .filter(|v| !v.is_empty())
                .context("未在 Flow Music 页面检测到已登录邮箱")?;
            if !actual.eq_ignore_ascii_case(&expected_email) {
                bail!("Flow Music 登录账号已切换为 {actual}，请重新连接");
            }
            let credits_payload = music_api(&peer, "GET", "/billing/credits", None).await?;
            let credits = parse_credits(&credits_payload)?;
            let plan = music_api(&peer, "GET", "/billing/subscription", None)
                .await
                .map(|v| parse_plan(&v))
                .unwrap_or_default();
            let current = self.music_bridge.get(&account.id).context("连接已关闭")?;
            if !Arc::ptr_eq(&current, &peer) {
                bail!("连接已被替换");
            }
            account = self.store.account(&account.id)?;
            if account.email != expected_email {
                bail!("账号配置已改变，请重新连接");
            }
            account.music_credits = Some(credits);
            if !plan.is_empty() {
                account.music_plan = plan;
            }
            account.music_last_synced = Some(now());
            self.store.save_account(&account)?;
            self.refresh()?;
            Ok::<_, anyhow::Error>(())
        }
        .await;
        match outcome {
            Ok(()) => peer.set_state("ready", "Flow Music 身份与积分校验通过，可以创作音乐"),
            Err(error) => {
                peer.set_state("failed", &error.to_string());
                self.change_status(|s| s.last_error = Some(error.to_string()));
            }
        }
        let _ = self.refresh();
    }

    pub async fn sync_music_account(&self, id: &str) -> Result<Account> {
        let _admission = self.job_gate.lock().await;
        self.ensure_available()?;
        if self.status().busy() {
            bail!("请等待生成任务完成后同步 Flow Music");
        }
        let mut account = self.store.account(id)?;
        let peer = self
            .music_bridge
            .get(id)
            .filter(|p| p.online())
            .context("Flow Music 标签页未连接，请在默认浏览器打开 flowmusic.app")?;
        let _session = peer.session.lock().await;
        let credits_payload = music_api(&peer, "GET", "/billing/credits", None).await?;
        account.music_credits = Some(parse_credits(&credits_payload)?);
        if let Ok(sub) = music_api(&peer, "GET", "/billing/subscription", None).await {
            let plan = parse_plan(&sub);
            if !plan.is_empty() {
                account.music_plan = plan;
            }
        }
        account.music_last_synced = Some(now());
        self.store.save_account(&account)?;
        peer.set_state("ready", "Flow Music 身份与积分校验通过，可以创作音乐");
        self.refresh()?;
        Ok(account)
    }

    pub(crate) async fn generate_music(
        &self,
        task: &Task,
        model: &Value,
        settings: &Settings,
        account: &mut Account,
        submitted: &mut bool,
    ) -> Result<Vec<Media>> {
        *submitted = false;
        let peer = self
            .music_bridge
            .get(&account.id)
            .filter(|p| p.online())
            .context("Flow Music 标签页未连接，请在默认浏览器打开 flowmusic.app 并保持页面打开")?;
        let _session = peer.session.lock().await;

        self.stage(&task.id, "校验 Flow Music 登录状态")?;
        let identity = peer.request("identity", Value::Null).await?;
        let actual = identity["email"]
            .as_str()
            .map(|v| v.trim().to_lowercase())
            .filter(|v| !v.is_empty())
            .context("Flow Music 页面未检测到已登录邮箱")?;
        if !actual.eq_ignore_ascii_case(&account.email) {
            bail!("Flow Music 登录账号已切换为 {actual}，请重新连接");
        }

        let cfg = &model["config"];
        let model_name = cfg["model_name"].as_str().unwrap_or("producer:standard");
        let instrumental = task.request.instrumental
            || cfg["instrumental"].as_bool().unwrap_or(false)
            || model["capabilities"]["kind"] == "music_instrumental";
        let message = build_message(
            &task.request.prompt,
            task.request.duration_seconds,
            instrumental,
            task.request.lyrics.as_deref(),
        );

        self.stage(&task.id, "创建 Flow Music 会话")?;
        let conv = music_api(
            &peer,
            "POST",
            "/conversation/create",
            Some(json!({"project_id": null})),
        )
        .await?;
        let cid = conv["conversation_id"]
            .as_str()
            .filter(|s| !s.is_empty())
            .context("Flow Music 未返回 conversation_id")?
            .to_string();

        self.stage(&task.id, "提交音乐创作请求")?;
        music_api(
            &peer,
            "POST",
            "/conversation",
            Some(json!({
                "conversation_id": cid,
                "parts": [{"content": message, "part_kind": "user-prompt"}],
                "client_context": {
                    "song_queue": [],
                    "lyrics_id_map": {},
                    "ghostwriter_version": "standard",
                    "client_platform": "web"
                },
                "model_name": model_name,
                "mode": "standard"
            })),
        )
        .await?;
        *submitted = true;

        self.stage(&task.id, "Flow Music 正在编曲与作词")?;
        let mut operation_id: Option<String> = None;
        let mut clip_id: Option<String> = None;
        let mut title = String::new();
        let mut lyrics = String::new();
        let mut sound_prompt = String::new();

        loop {
            let conv = music_api(&peer, "GET", &format!("/conversations/{cid}"), None).await?;
            let mut reply = String::new();
            for m in conv["messages"].as_array().into_iter().flatten() {
                for p in m["parts"].as_array().into_iter().flatten() {
                    let kind = p["part_kind"].as_str().unwrap_or_default();
                    let tool = p["tool_name"].as_str().unwrap_or_default();
                    if kind == "tool-return" && tool == "audio__create_song" {
                        if let Some(op) = p["content"]["operation_id"]
                            .as_str()
                            .filter(|s| !s.is_empty())
                        {
                            operation_id = Some(op.to_string());
                            if let Some(c) =
                                p["content"]["clip_id"].as_str().filter(|s| !s.is_empty())
                            {
                                clip_id = Some(c.to_string());
                            }
                            if let Some(t) =
                                p["content"]["title"].as_str().filter(|s| !s.is_empty())
                            {
                                title = t.to_string();
                            }
                        }
                    } else if kind == "tool-return" && tool == "lyrics__create" {
                        if let Some(l) = p["content"]["lyrics"].as_str().filter(|s| !s.is_empty()) {
                            lyrics = l.to_string();
                        }
                    } else if kind == "tool-call" && tool == "audio__create_song" {
                        let args = match &p["args"] {
                            Value::String(s) => {
                                serde_json::from_str::<Value>(s).unwrap_or(Value::Null)
                            }
                            other => other.clone(),
                        };
                        if let Some(sp) = args["sound_prompt"].as_str().filter(|s| !s.is_empty()) {
                            sound_prompt = sp.to_string();
                        }
                        if title.is_empty() {
                            if let Some(t) = args["title"].as_str().filter(|s| !s.is_empty()) {
                                title = t.to_string();
                            }
                        }
                    } else if kind == "text" {
                        if let Some(c) = p["content"].as_str() {
                            reply = c.to_string();
                        }
                    }
                }
            }
            if operation_id.is_some() {
                break;
            }
            let running = music_api(
                &peer,
                "GET",
                &format!("/conversations/{cid}/running-job"),
                None,
            )
            .await?;
            if running["job_id"].is_null() && !reply.trim().is_empty() {
                bail!(
                    "Flow Music 未生成歌曲，回复：{}",
                    reply.trim().chars().take(240).collect::<String>()
                );
            }
            sleep(Duration::from_secs(4)).await;
        }

        let op = operation_id.context("Flow Music 未返回音频渲染任务 ID")?;
        self.stage(&task.id, "Flow Music 正在渲染音频")?;
        let final_clip = loop {
            let status = music_api(
                &peer,
                "GET",
                &format!("/audio-create-song-status/{op}"),
                None,
            )
            .await?;
            let state = status["status"].as_str().unwrap_or_default();
            if state == "complete" {
                let id = status["clip_id"]
                    .as_str()
                    .filter(|s| !s.is_empty())
                    .map(str::to_owned)
                    .or(clip_id.clone())
                    .context("Flow Music 音频完成但未返回 clip_id")?;
                break id;
            }
            if !status["error_type"].is_null() || state == "failed" || state == "error" {
                let detail = status["error_type"]
                    .as_str()
                    .or_else(|| status["message"].as_str())
                    .unwrap_or("渲染失败");
                bail!("Flow Music 音频生成失败：{detail}");
            }
            sleep(Duration::from_secs(5)).await;
        };

        self.stage(&task.id, "下载并保存音频到本机")?;
        let url = format!("{CLIP_BASE_URL}/{final_clip}.m4a");
        let response = http_client(settings)?.get(&url).send().await.map_err(|_| {
            anyhow::anyhow!("音频作品下载失败，请检查网络或代理；clip_id={final_clip}")
        })?;
        if !response.status().is_success() {
            bail!(
                "音频作品下载 HTTP {}（clip_id={final_clip}）",
                response.status().as_u16()
            );
        }
        let mut bytes = Vec::new();
        let mut stream = response.bytes_stream();
        while let Some(chunk) = stream.next().await {
            bytes.extend_from_slice(&chunk.map_err(|_| anyhow::anyhow!("音频下载中断"))?);
            if bytes.len() > 256 * 1024 * 1024 {
                bail!("音频超出 256 MB 保存限制");
            }
        }
        let media = self
            .archive_bytes_with_meta(
                &bytes,
                "audio",
                &task.id,
                &final_clip,
                title,
                lyrics,
                sound_prompt,
            )
            .await?;

        // Best-effort refresh remaining music credits after generation.
        if let Ok(credits_payload) = music_api(&peer, "GET", "/billing/credits", None).await {
            if let Ok(credits) = parse_credits(&credits_payload) {
                account.music_credits = Some(credits);
                account.music_last_synced = Some(now());
                let _ = self.store.save_account(account);
            }
        }
        Ok(vec![media])
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{
        body::{to_bytes, Body},
        http::{Request, StatusCode},
    };
    use tower::ServiceExt;

    #[test]
    fn builds_vocal_and_instrumental_messages() {
        let vocal = build_message("Upbeat pop", Some(62), false, Some("[Verse]\nHello"));
        assert!(vocal.contains("Upbeat pop"));
        assert!(vocal.contains("Target length: about 62 seconds."));
        assert!(vocal.contains("Use exactly these lyrics, word for word:\n[Verse]\nHello"));
        assert!(!vocal.contains("Instrumental only"));

        let inst = build_message("Guofeng score", Some(45), true, Some("ignored"));
        assert!(inst.contains("Instrumental only: no vocals, no lyrics."));
        assert!(!inst.contains("ignored"));
    }

    #[tokio::test]
    async fn music_bridge_pairs_and_validates_credits() -> Result<()> {
        let dir = tempfile::tempdir()?;
        let app = App::open(dir.path())?;
        let (id, token) = app.pair_music_browser("singer@example.test").await?;
        assert_eq!(app.status().music_ready_accounts, 0);

        let mut result: Option<Value> = None;
        for _ in 0..200 {
            let response = crate::api::router(app.clone())
                .oneshot(
                    Request::builder()
                        .method("POST")
                        .uri("/api/music-bridge/poll")
                        .header("Content-Type", "application/json")
                        .header("Authorization", format!("Bearer {token}"))
                        .body(Body::from(json!({"result": result.take()}).to_string()))?,
                )
                .await?;
            assert_eq!(response.status(), StatusCode::OK);
            let body: Value =
                serde_json::from_slice(&to_bytes(response.into_body(), 1024 * 1024).await?)?;
            if body["connection"]["state"] != "validating" {
                assert_eq!(body["connection"]["state"], "ready");
                break;
            }
            if let Some(command) = body["commands"].as_array().and_then(|a| a.first()) {
                let value = match command["operation"].as_str().unwrap() {
                    "identity" => json!({"email": "singer@example.test"}),
                    "music_api" => match command["input"]["path"].as_str().unwrap() {
                        "/billing/credits" => {
                            json!({"status": 200, "json": {"data": {"credits_remaining": 30620}}})
                        }
                        "/billing/subscription" => {
                            json!({"status": 200, "json": {"data": {"subscription_tier": "member"}}})
                        }
                        other => bail!("unexpected path {other}"),
                    },
                    other => bail!("unexpected operation {other}"),
                };
                result = Some(json!({"id": command["id"], "value": value}));
            }
            sleep(Duration::from_millis(1)).await;
        }

        let account = app.store.account(&id)?;
        assert_eq!(account.music_credits, Some(30620));
        assert_eq!(account.music_plan, "member");
        assert!(account.music_last_synced.is_some());
        assert_eq!(app.status().music_ready_accounts, 1);
        Ok(())
    }
}
