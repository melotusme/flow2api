use crate::{browser::Session, catalog, flow, model::*, App};
use anyhow::{bail, Context, Result};
use base64::{engine::general_purpose::STANDARD, Engine};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::{sync::Arc, time::Duration};
use tokio::time::{sleep, timeout};

pub fn validate(request: &GenerationRequest) -> Result<Value> {
    let model = catalog::model(&request.model)?;
    let capabilities = &model["capabilities"];
    if request.prompt.trim().is_empty() {
        bail!("请输入提示词");
    }
    if request.prompt.len() > 100_000 {
        bail!("提示词过长");
    }
    if request.images.len() < capabilities["min_images"].as_u64().unwrap_or(0) as usize
        || request.images.len() > capabilities["max_images"].as_u64().unwrap_or(0) as usize
    {
        bail!("参考图数量与模型要求不符");
    }
    if capabilities["requires_video_id"] == true
        && request.video_id.as_deref().unwrap_or_default().is_empty()
    {
        bail!("请填写续写所需的源视频 ID");
    }
    Ok(model)
}

impl App {
    pub(crate) async fn verify_login(
        &self,
        session: &mut Session,
        account: &mut Account,
    ) -> Result<()> {
        if let Err(error) = session.verify_account(&account.email).await {
            // A historical sync must not keep an expired or mismatched account eligible.
            account.last_synced = None;
            self.store.save_account(account)?;
            self.refresh()?;
            self.change_status(|s| {
                s.browser = format!("{} 需要重新登录同步", account.email);
                s.last_error = Some(error.to_string());
            });
            return Err(error);
        }
        Ok(())
    }
    pub async fn open_login(&self, id: &str) -> Result<()> {
        let _admission = self.job_gate.lock().await;
        self.ensure_available()?;
        if self.status().busy() {
            bail!("请等待生成任务完成后打开登录窗口");
        }
        let account = self.store.account(id)?;
        let settings = self.store.settings()?;
        let session = self
            .browsers
            .session(
                id,
                &settings.browser_executable,
                &settings.proxy_url,
                &account.browser_data_dir,
                &account.browser_profile,
            )
            .await?;
        session
            .lock()
            .await
            .call("Page.bringToFront", json!({}))
            .await?;
        self.change_status(|s| s.browser = "Chrome 已打开，等待登录／同步".into());
        Ok(())
    }
    pub async fn sync_account(&self, id: &str) -> Result<Account> {
        let _admission = self.job_gate.lock().await;
        self.ensure_available()?;
        if self.status().busy() {
            bail!("请等待生成任务完成后同步账号");
        }
        let mut account = self.store.account(id)?;
        let settings = self.store.settings()?;
        let session = match self
            .browsers
            .session(
                id,
                &settings.browser_executable,
                &settings.proxy_url,
                &account.browser_data_dir,
                &account.browser_profile,
            )
            .await
        {
            Ok(s) => s,
            Err(e) if !account.browser_data_dir.is_empty() => {
                bail!(
                    "{}。若日常 Chrome 正在运行，请在账号卡片中点击「改为独立登录」或通过 Chrome 扩展「同步到 Flow2API」，无需退出日常浏览器。",
                    e
                );
            }
            Err(e) => return Err(e),
        };
        let mut session = session.lock().await;
        self.verify_login(&mut session, &mut account).await?;
        let credits = flow::rpc(&mut session, "nzlxg", json!([]), None).await?;
        account.credits = credits
            .as_array()
            .and_then(|a| a.iter().find_map(Value::as_i64));
        account.last_synced = Some(now());
        self.store.save_account(&account)?;
        self.change_status(|s| {
            s.browser = format!("已连接 {}", account.email);
            s.last_error = None;
        });
        self.refresh()?;
        Ok(account)
    }
    /// One-click login: open a fresh standalone Chrome profile on the Google sign-in page
    /// without asking for an email first. Returns the pending id; the account is only
    /// persisted once `try_finish_quick_login` detects the signed-in email.
    pub async fn start_quick_login(&self) -> Result<String> {
        let _admission = self.job_gate.lock().await;
        self.ensure_available()?;
        if self.status().busy() {
            bail!("请等待生成任务完成后登录账号");
        }
        let id = uuid::Uuid::new_v4().to_string();
        let settings = self.store.settings()?;
        let session = self
            .browsers
            .session(
                &id,
                &settings.browser_executable,
                &settings.proxy_url,
                "",
                "",
            )
            .await?;
        let mut session = session.lock().await;
        session
            .navigate("https://accounts.google.com/ServiceLogin?continue=https%3A%2F%2Fflow.google.com%2Fprojects")
            .await
            .ok();
        session.call("Page.bringToFront", json!({})).await?;
        self.change_status(|s| s.browser = "Chrome 已打开，等待 Google 登录".into());
        Ok(id)
    }
    /// Ok(None): still waiting. Ok(Some): account saved (synced if credits were readable).
    /// Err: login detected but cannot be used (e.g. account already added).
    pub async fn try_finish_quick_login(&self, id: &str) -> Result<Option<Account>> {
        let Ok(_admission) = self.job_gate.try_lock() else {
            return Ok(None);
        };
        if self.ensure_available().is_err() || self.status().busy() {
            return Ok(None);
        }
        let Some(session) = self.browsers.existing_session(id).await else {
            return Ok(None);
        };
        let mut session = session.lock().await;
        let Ok(Some(email)) = session.detect_email().await else {
            return Ok(None);
        };
        if self
            .store
            .accounts()?
            .iter()
            .any(|a| a.email.eq_ignore_ascii_case(&email))
        {
            bail!("{email} 已在账号列表中，请直接在该账号上点“打开登录窗口”");
        }
        let mut account = Account {
            id: id.into(),
            email,
            enabled: true,
            project_id: String::new(),
            last_synced: None,
            credits: None,
            browser_data_dir: String::new(),
            browser_profile: String::new(),
        };
        if let Ok(credits) = flow::rpc(&mut session, "nzlxg", json!([]), None).await {
            account.credits = credits
                .as_array()
                .and_then(|a| a.iter().find_map(Value::as_i64));
            account.last_synced = Some(now());
        }
        self.store.save_account(&account)?;
        self.change_status(|s| {
            s.browser = format!("已连接 {}", account.email);
            s.last_error = None;
        });
        self.refresh()?;
        Ok(Some(account))
    }
    /// Import cookies exported from the Chrome extension or UserScript.
    /// Injects cookies into a dedicated browser profile via CDP, navigates to Flow,
    /// verifies login / reads credits, and saves the account.
    pub async fn sync_cookies(&self, email: Option<&str>, cookies: &[Value]) -> Result<Account> {
        let _admission = self.job_gate.lock().await;
        self.ensure_available()?;
        if self.status().busy() {
            bail!("请等待当前生成任务完成后导入账号");
        }
        let mut cdp_cookies = Vec::new();
        for c in cookies {
            let Some(name) = c.get("name").and_then(|v| v.as_str()) else {
                continue;
            };
            let Some(value) = c.get("value").and_then(|v| v.as_str()) else {
                continue;
            };
            let mut obj = json!({
                "name": name,
                "value": value,
            });
            if let Some(domain) = c.get("domain").and_then(|v| v.as_str()) {
                obj["domain"] = json!(domain);
            }
            if let Some(path) = c.get("path").and_then(|v| v.as_str()) {
                obj["path"] = json!(path);
            }
            if let Some(secure) = c.get("secure").and_then(|v| v.as_bool()) {
                obj["secure"] = json!(secure);
            }
            if let Some(http_only) = c
                .get("httpOnly")
                .or_else(|| c.get("http_only"))
                .and_then(|v| v.as_bool())
            {
                obj["httpOnly"] = json!(http_only);
            }
            if let Some(expires) = c
                .get("expirationDate")
                .or_else(|| c.get("expires"))
                .and_then(|v| v.as_f64())
            {
                obj["expires"] = json!(expires);
            }
            cdp_cookies.push(obj);
        }
        if cdp_cookies.is_empty() {
            bail!("未找到有效的 Cookie 数据");
        }

        let existing = if let Some(e) = email.filter(|s| !s.trim().is_empty()) {
            self.store
                .accounts()?
                .into_iter()
                .find(|a| a.email.eq_ignore_ascii_case(e.trim()))
        } else {
            None
        };

        let id = existing
            .as_ref()
            .map(|a| a.id.clone())
            .unwrap_or_else(|| uuid::Uuid::new_v4().to_string());
        let settings = self.store.settings()?;
        // Cookie sync injects cookies into Flow2API's dedicated standalone profile so that
        // it never locks or conflicts with everyday Chrome instances.
        let session = self
            .browsers
            .session(
                &id,
                &settings.browser_executable,
                &settings.proxy_url,
                "",
                "",
            )
            .await?;

        let mut session = session.lock().await;
        session
            .call("Network.setCookies", json!({ "cookies": cdp_cookies }))
            .await?;
        session.navigate("https://flow.google.com/projects").await?;

        let mut detected_email = None;
        for _ in 0..15 {
            if let Ok(Some(e)) = session.detect_email().await {
                detected_email = Some(e);
                break;
            }
            tokio::time::sleep(Duration::from_millis(500)).await;
        }

        let target_email = match (detected_email, email) {
            (Some(e), _) => e,
            (None, Some(e)) if !e.trim().is_empty() => {
                session.verify_account(e.trim()).await?;
                e.trim().to_lowercase()
            }
            _ => bail!("未能从 Flow 页面识别到登录账号，请确认已在浏览器中正常登录 Flow"),
        };

        let credits = flow::rpc(&mut session, "nzlxg", json!([]), None)
            .await
            .ok()
            .and_then(|c| c.as_array().and_then(|a| a.iter().find_map(Value::as_i64)));

        let account = match self
            .store
            .accounts()?
            .into_iter()
            .find(|a| a.email.eq_ignore_ascii_case(&target_email))
        {
            Some(mut acc) => {
                acc.credits = credits;
                acc.last_synced = Some(now());
                acc.browser_data_dir = String::new();
                acc.browser_profile = String::new();
                acc
            }
            None => Account {
                id: id.clone(),
                email: target_email.clone(),
                enabled: true,
                project_id: String::new(),
                last_synced: Some(now()),
                credits,
                browser_data_dir: String::new(),
                browser_profile: String::new(),
            },
        };

        self.store.save_account(&account)?;
        self.change_status(|s| {
            s.browser = format!("已从扩展连接 {}", account.email);
            s.last_error = None;
        });
        self.refresh()?;
        Ok(account)
    }
    pub async fn try_auto_sync(&self, id: &str) -> Result<Option<Account>> {
        let Ok(_admission) = self.job_gate.try_lock() else {
            return Ok(None);
        };
        if self.ensure_available().is_err() || self.status().busy() {
            return Ok(None);
        }
        let Some(session) = self.browsers.existing_session(id).await else {
            return Ok(None);
        };
        let mut account = self.store.account(id)?;
        let mut session = session.lock().await;
        if session.verify_account(&account.email).await.is_err() {
            return Ok(None);
        }
        let Ok(credits) = flow::rpc(&mut session, "nzlxg", json!([]), None).await else {
            return Ok(None);
        };
        account.credits = credits
            .as_array()
            .and_then(|a| a.iter().find_map(Value::as_i64));
        account.last_synced = Some(now());
        self.store.save_account(&account)?;
        self.change_status(|s| {
            s.browser = format!("已连接 {}", account.email);
            s.last_error = None;
        });
        self.refresh()?;
        Ok(Some(account))
    }
    pub async fn submit(self: &Arc<Self>, mut request: GenerationRequest) -> Result<Task> {
        let _admission = self.job_gate.lock().await;
        self.ensure_available()?;
        if self.status().service != "running" {
            bail!("请先启动 API 服务");
        }
        validate(&request)?;
        if self.status().queued > 128 {
            bail!("排队任务过多，请稍后提交");
        }
        let accounts = self.store.accounts()?;
        let account = match request.account_id.as_deref() {
            Some(id) => accounts
                .into_iter()
                .find(|a| a.id == id && a.enabled && a.last_synced.is_some()),
            None => {
                let tasks = self.store.tasks()?;
                accounts
                    .into_iter()
                    .filter(|a| a.enabled && a.last_synced.is_some())
                    .min_by_key(|a| account_load(a, &tasks))
            }
        }
        .context("没有已启用且完成登录同步的账号")?;
        request.account_id = Some(account.id);
        let task = Task {
            id: uuid::Uuid::new_v4().to_string(),
            request,
            state: "queued".into(),
            stage: "等待执行".into(),
            created_at: now(),
            updated_at: now(),
            error: None,
            results: vec![],
        };
        self.store.save_task(&task)?;
        self.refresh()?;
        let app = self.clone();
        let id = task.id.clone();
        tokio::spawn(async move {
            let result = app.run_task(&id).await;
            if let Err(error) = result {
                if let Ok(mut task) = app.store.task(&id) {
                    task.state = "failed".into();
                    task.stage = "执行失败，请查看详情".into();
                    task.error = Some(error.to_string());
                    task.updated_at = now();
                    let _ = app.store.save_task(&task);
                }
                app.change_status(|s| s.last_error = Some(error.to_string()));
            }
            let _ = app.refresh();
        });
        Ok(task)
    }
    pub fn stage(&self, id: &str, stage: &str) -> Result<()> {
        let mut task = self.store.task(id)?;
        task.stage = stage.into();
        task.updated_at = now();
        self.store.save_task(&task)?;
        self.refresh()
    }
    async fn run_task(&self, id: &str) -> Result<()> {
        let _permit = self.workers.acquire().await?;
        let mut task = self.store.task(id)?;
        if task.state != "queued" {
            return Ok(());
        }
        task.state = "running".into();
        task.stage = "连接专用 Chrome".into();
        task.updated_at = now();
        self.store.save_task(&task)?;
        self.refresh()?;
        let settings = self.store.settings()?;
        let model = validate(&task.request)?;
        let timeout_seconds = if model["config"]["type"] == "image" {
            settings.image_timeout_seconds
        } else {
            settings.video_timeout_seconds
        };
        let media = timeout(
            Duration::from_secs(timeout_seconds),
            self.generate(&task, &model, &settings),
        )
        .await
        .context("生成超时，Flow 可能仍在执行；请核对上游结果后决定是否重试")??;
        task.results = media;
        task.state = "succeeded".into();
        task.stage = "已完成并保存到本机".into();
        task.updated_at = now();
        self.store.save_task(&task)?;
        self.refresh()
    }
    async fn generate(
        &self,
        task: &Task,
        model: &Value,
        settings: &Settings,
    ) -> Result<Vec<Media>> {
        let mut account = self
            .store
            .account(task.request.account_id.as_deref().context("任务缺少账号")?)?;
        let session = self
            .browsers
            .session(
                &account.id,
                &settings.browser_executable,
                &settings.proxy_url,
                &account.browser_data_dir,
                &account.browser_profile,
            )
            .await?;
        let mut session = session.lock().await;
        self.verify_login(&mut session, &mut account).await?;
        if account.project_id.is_empty() {
            self.stage(&task.id, "创建 Flow 项目")?;
            let payload = flow::rpc(
                &mut session,
                "jHPbke",
                json!(["projects/*", [null, ["Flow2API Desktop"]], [null, 22]]),
                None,
            )
            .await?;
            account.project_id = flow::find_uuid(&payload, &[]).context("创建项目未返回项目 ID")?;
            self.store.save_account(&account)?;
        }
        session.project(&account.project_id, &account.email).await?;
        let mut references = vec![];
        for image in &task.request.images {
            self.stage(&task.id, "上传参考图")?;
            let (bytes, mime) = load_image(image, settings).await?;
            let captcha = session.captcha("UPLOAD_IMAGE").await?;
            let argument = json!([
                flow::context(&account.project_id, &captcha),
                STANDARD.encode(bytes),
                mime,
                1,
                null,
                null,
                null,
                null,
                format!("{}.jpg", uuid::Uuid::new_v4()),
                null,
                uuid::Uuid::new_v4().to_string().to_uppercase(),
                uuid::Uuid::new_v4().to_string().to_uppercase()
            ]);
            let payload =
                flow::rpc(&mut session, "maseQ", argument, Some(&account.project_id)).await?;
            references.push(
                flow::find_uuid(&payload, &[&account.project_id])
                    .context("参考图上传未返回媒体 ID")?,
            );
        }
        let cfg = &model["config"];
        let aspect = cfg["aspect_ratio"].as_str().context("模型缺少画面比例")?;
        if cfg["type"] == "image" {
            self.stage(&task.id, "获取图片验证码")?;
            let captcha = session.captcha("IMAGE_GENERATION").await?;
            self.stage(&task.id, "提交图片生成，等待 Flow 返回")?;
            let argument = flow::image_argument(
                &account.project_id,
                &task.request.prompt,
                cfg["model_name"].as_str().context("模型缺少上游名称")?,
                aspect,
                &captcha,
                &references,
            );
            let payload =
                flow::rpc(&mut session, "ogiZ0b", argument, Some(&account.project_id)).await?;
            let items = payload[0].as_array().context("图片生成未返回媒体")?;
            let mut results = vec![];
            for item in items {
                let mut url = flow::media_url(item, "image").context("图片生成未返回下载地址")?;
                let media_id = item[0].as_str().context("图片缺少媒体 ID")?;
                if let Some(upsample) = cfg["upsample"].as_str() {
                    self.stage(&task.id, "图片高清放大")?;
                    let captcha = session.captcha("IMAGE_GENERATION").await?;
                    let payload = flow::rpc(
                        &mut session,
                        "SPrCad",
                        json!([
                            media_id,
                            if upsample.contains("4K") { 2 } else { 1 },
                            flow::context(&account.project_id, &captcha)
                        ]),
                        Some(&account.project_id),
                    )
                    .await?;
                    if let Some(bytes) = encoded_image(&payload) {
                        self.stage(&task.id, "保存高清图片到本机")?;
                        results.push(
                            self.archive_bytes(&bytes, "image", &task.id, media_id)
                                .await?,
                        );
                        continue;
                    }
                    url = flow::media_url(&payload, "image")
                        .context("图片放大未返回有效图片或下载地址")?;
                }
                self.stage(&task.id, "保存图片到本机")?;
                results.push(
                    self.archive(&url, "image", &task.id, media_id, settings)
                        .await?,
                );
            }
            if results.is_empty() {
                bail!("图片生成没有返回任何作品");
            }
            Ok(results)
        } else {
            let kind = cfg["video_type"].as_str().unwrap_or("t2v");
            let mode = match kind {
                "omni" | "r2v" => {
                    if references.is_empty() {
                        "text"
                    } else {
                        "references"
                    }
                }
                "i2v" => {
                    if references.len() == 2 {
                        "start_end"
                    } else {
                        "start"
                    }
                }
                "extend" => "extend",
                _ => "text",
            };
            let key = if kind == "omni" && !references.is_empty() {
                cfg["reference_model_key"].as_str()
            } else {
                cfg["model_key"].as_str()
            }
            .context("视频模型缺少上游名称")?;
            self.stage(&task.id, "获取视频验证码")?;
            let captcha = session.captcha("VIDEO_GENERATION").await?;
            let (rpc, argument) = flow::video_argument(
                &account.project_id,
                &task.request.prompt,
                key,
                aspect,
                &captcha,
                mode,
                &references,
                task.request.video_id.as_deref(),
                None,
            )?;
            self.stage(&task.id, "提交视频任务")?;
            let payload = flow::rpc(&mut session, rpc, argument, Some(&account.project_id)).await?;
            let videos = self
                .poll_video(&mut session, task, &account.project_id, payload)
                .await?;
            let mut results = vec![];
            for video in videos {
                let videos = if cfg["upsample"].is_object() {
                    if video.media_id.is_empty() {
                        bail!("视频返回了下载地址但缺少媒体 ID，无法放大；请核对 Flow 结果");
                    }
                    let upsample = &cfg["upsample"];
                    let resolution = upsample["resolution"]
                        .as_str()
                        .context("视频放大配置缺少分辨率")?;
                    self.stage(&task.id, "视频高清放大")?;
                    let captcha = session.captcha("VIDEO_GENERATION").await?;
                    let (rpc, argument) = flow::video_argument(
                        &account.project_id,
                        "",
                        upsample["model_key"].as_str().unwrap_or(""),
                        aspect,
                        &captcha,
                        "upsample",
                        &[],
                        Some(&video.media_id),
                        Some(resolution),
                    )?;
                    let payload =
                        flow::rpc(&mut session, rpc, argument, Some(&account.project_id)).await?;
                    self.poll_video(&mut session, task, &account.project_id, payload)
                        .await?
                } else {
                    vec![video]
                };
                for video in videos {
                    self.stage(&task.id, "保存视频到本机")?;
                    results.push(
                        self.archive(&video.url, "video", &task.id, &video.media_id, settings)
                            .await?,
                    );
                }
            }
            Ok(results)
        }
    }
    async fn poll_video(
        &self,
        session: &mut Session,
        task: &Task,
        project: &str,
        payload: Value,
    ) -> Result<Vec<crate::video::VideoResult>> {
        let mut batch = crate::video::VideoBatch::submitted(&payload, project)?;
        loop {
            if let Some(results) = batch.results() {
                return Ok(results);
            }
            self.stage(&task.id, "Flow 正在生成视频")?;
            sleep(Duration::from_secs(3)).await;
            let operations: Vec<Value> = batch.pending().iter().map(|id| json!([id])).collect();
            let payload = flow::rpc(
                session,
                "jwpduf",
                json!([null, null, operations]),
                Some(project),
            )
            .await?;
            batch.update(&payload, None)?;
            for operation in batch.pending() {
                // Read-only lookup: never resubmit generation to recover a URL.
                if let Ok(media) =
                    flow::rpc(session, "as29s", json!([operation]), Some(project)).await
                {
                    batch.update(&media, Some(&operation))?;
                }
            }
        }
    }
    async fn archive(
        &self,
        url: &str,
        kind: &str,
        task_id: &str,
        media_id: &str,
        settings: &Settings,
    ) -> Result<Media> {
        let client = http_client(settings)?;
        let response = client
            .get(url)
            .send()
            .await
            .map_err(|_| anyhow::anyhow!("作品下载失败，请检查网络或代理；任务未标记成功"))?;
        if !response.status().is_success() {
            bail!("作品下载 HTTP {}，未标记成功", response.status().as_u16());
        }
        let mut stream = response.bytes_stream();
        use futures_util::StreamExt;
        let mut bytes = vec![];
        while let Some(chunk) = stream.next().await {
            bytes.extend_from_slice(&chunk.map_err(|_| anyhow::anyhow!("作品下载中断"))?);
            if bytes.len() > 512 * 1024 * 1024 {
                bail!("作品超出 512 MB 保存限制");
            }
        }
        self.archive_bytes(&bytes, kind, task_id, media_id).await
    }
    async fn archive_bytes(
        &self,
        bytes: &[u8],
        kind: &str,
        task_id: &str,
        media_id: &str,
    ) -> Result<Media> {
        if bytes.len() > 512 * 1024 * 1024 {
            bail!("作品超出 512 MB 保存限制");
        }
        let extension = media_extension(bytes, kind)?;
        let filename = format!("{:x}.{extension}", Sha256::digest(&bytes));
        let directory = self.store.root.join("generated");
        let target = directory.join(&filename);
        if !target.exists() {
            let temporary = directory.join(format!("{}.tmp", uuid::Uuid::new_v4()));
            tokio::fs::write(&temporary, &bytes)
                .await
                .context("作品写入失败，请检查磁盘空间")?;
            tokio::fs::rename(&temporary, &target)
                .await
                .context("作品归档失败")?;
        }
        let (prompt, model) = self
            .store
            .task(task_id)
            .map(|t| {
                (
                    t.request.prompt.chars().take(2000).collect::<String>(),
                    t.request.model,
                )
            })
            .unwrap_or_default();
        let media = Media {
            filename,
            kind: kind.into(),
            size: bytes.len() as u64,
            created_at: now(),
            task_id: task_id.into(),
            media_id: media_id.into(),
            prompt,
            model,
        };
        tokio::fs::write(
            target.with_extension("json"),
            serde_json::to_vec_pretty(&media)?,
        )
        .await?;
        self.store.save_media(&media)?;
        Ok(media)
    }
}

fn account_load(account: &Account, tasks: &[Task]) -> (usize, String) {
    let assigned = tasks
        .iter()
        .filter(|t| t.request.account_id.as_deref() == Some(&account.id));
    let busy = assigned
        .clone()
        .filter(|t| ["running", "queued"].contains(&t.state.as_str()))
        .count();
    let last_used = assigned
        .map(|t| t.created_at.as_str())
        .max()
        .unwrap_or_default()
        .to_owned();
    (busy, last_used)
}

fn encoded_image(value: &Value) -> Option<Vec<u8>> {
    match value {
        Value::String(text) if text.len() <= 128 * 1024 * 1024 => {
            let text = text
                .strip_prefix("data:image/")
                .and_then(|v| v.split_once(",").map(|(_, data)| data))
                .unwrap_or(text);
            if text.starts_with("https://") {
                return None;
            }
            use base64::engine::general_purpose::{STANDARD_NO_PAD, URL_SAFE, URL_SAFE_NO_PAD};
            let bytes = STANDARD
                .decode(text)
                .or_else(|_| STANDARD_NO_PAD.decode(text))
                .or_else(|_| URL_SAFE.decode(text))
                .or_else(|_| URL_SAFE_NO_PAD.decode(text))
                .ok()?;
            media_extension(&bytes, "image").ok()?;
            Some(bytes)
        }
        Value::Array(values) => values.iter().find_map(encoded_image),
        Value::Object(values) => values.values().find_map(encoded_image),
        _ => None,
    }
}

fn http_client(settings: &Settings) -> Result<reqwest::Client> {
    let mut builder = reqwest::Client::builder()
        .no_proxy()
        .timeout(Duration::from_secs(120));
    if !settings.proxy_url.is_empty() {
        builder = builder.proxy(reqwest::Proxy::all(&settings.proxy_url).context("代理配置无效")?);
    }
    Ok(builder.build()?)
}
pub(crate) async fn load_image(image: &str, settings: &Settings) -> Result<(Vec<u8>, String)> {
    let bytes = if image.starts_with("data:image/") {
        if image.len() > (20usize * 1024 * 1024).div_ceil(3) * 4 + 1024 {
            bail!("参考图超过 20 MB");
        }
        STANDARD
            .decode(image.split_once(',').context("图片 data URI 格式错误")?.1)
            .context("图片 base64 格式错误")?
    } else {
        let url = url::Url::parse(image).context("参考图 URL 格式错误")?;
        if !["http", "https"].contains(&url.scheme()) {
            bail!("参考图只支持 HTTP(S) 或 image data URI");
        }
        let response = http_client(settings)?
            .get(url)
            .send()
            .await
            .map_err(|_| anyhow::anyhow!("参考图下载失败"))?;
        if !response.status().is_success()
            || response.content_length().unwrap_or(0) > 20 * 1024 * 1024
        {
            bail!("参考图下载失败或超过 20 MB");
        }
        let mut bytes = Vec::new();
        let mut stream = response.bytes_stream();
        use futures_util::StreamExt;
        while let Some(chunk) = stream.next().await {
            let chunk = chunk.map_err(|_| anyhow::anyhow!("参考图读取失败"))?;
            if bytes.len() + chunk.len() > 20 * 1024 * 1024 {
                bail!("参考图超过 20 MB");
            }
            bytes.extend_from_slice(&chunk);
        }
        bytes
    };
    if bytes.len() > 20 * 1024 * 1024 {
        bail!("参考图超过 20 MB");
    }
    let ext = media_extension(&bytes, "image")?;
    Ok((
        bytes,
        match ext {
            "jpg" => "image/jpeg",
            "png" => "image/png",
            "webp" => "image/webp",
            _ => "image/gif",
        }
        .into(),
    ))
}
pub fn media_extension(bytes: &[u8], kind: &str) -> Result<&'static str> {
    if kind == "image" {
        if bytes.starts_with(&[0xff, 0xd8, 0xff]) {
            return Ok("jpg");
        }
        if bytes.starts_with(b"\x89PNG\r\n\x1a\n") {
            return Ok("png");
        }
        if bytes.starts_with(b"RIFF") && bytes.get(8..12) == Some(b"WEBP") {
            return Ok("webp");
        }
        if bytes.starts_with(b"GIF8") {
            return Ok("gif");
        }
    } else {
        if bytes.get(4..8) == Some(b"ftyp") {
            return Ok("mp4");
        }
        if bytes.starts_with(&[0x1a, 0x45, 0xdf, 0xa3]) {
            return Ok("webm");
        }
    }
    bail!("下载内容不是预期的图片／视频，未将错误页面保存为作品")
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn accounts_prefer_free_then_least_recently_used() {
        let account = |id: &str| Account {
            id: id.into(),
            email: format!("{id}@example.test"),
            enabled: true,
            project_id: String::new(),
            last_synced: Some(now()),
            credits: None,
            browser_data_dir: String::new(),
            browser_profile: String::new(),
        };
        let first = account("first");
        let second = account("second");
        let mut task = Task {
            id: "job".into(),
            request: GenerationRequest {
                model: String::new(),
                prompt: String::new(),
                images: vec![],
                account_id: Some(first.id.clone()),
                video_id: None,
            },
            state: "succeeded".into(),
            stage: String::new(),
            created_at: now(),
            updated_at: now(),
            error: None,
            results: vec![],
        };
        assert!(account_load(&second, &[task.clone()]) < account_load(&first, &[task.clone()]));
        task.state = "running".into();
        assert_eq!(account_load(&first, &[task.clone()]).0, 1);
        assert!(account_load(&second, &[task.clone()]) < account_load(&first, &[task]));
    }
    #[tokio::test]
    async fn upscale_encoded_image_is_validated_and_archived() -> Result<()> {
        let bytes = b"\x89PNG\r\n\x1a\nfixture";
        let payload = json!([null, [STANDARD.encode(bytes)]]);
        assert_eq!(encoded_image(&payload).as_deref(), Some(bytes.as_slice()));
        assert!(encoded_image(&json!(STANDARD.encode(b"<html>error</html>"))).is_none());
        let directory = tempfile::tempdir()?;
        let app = App::open(directory.path())?;
        app.store.save_task(&Task {
            id: "job".into(),
            request: GenerationRequest {
                model: "gemini-3.1-flash-image-square".into(),
                prompt: "yellow teapot".into(),
                images: vec![],
                account_id: None,
                video_id: None,
            },
            state: "running".into(),
            stage: String::new(),
            created_at: now(),
            updated_at: now(),
            error: None,
            results: vec![],
        })?;
        let media = app
            .archive_bytes(
                &encoded_image(&payload).unwrap(),
                "image",
                "job",
                "source-id",
            )
            .await?;
        assert_eq!(media.media_id, "source-id");
        assert_eq!(media.prompt, "yellow teapot");
        assert_eq!(media.model, "gemini-3.1-flash-image-square");
        assert_eq!(
            tokio::fs::read(directory.path().join("generated").join(media.filename)).await?,
            bytes
        );
        Ok(())
    }
    #[tokio::test]
    #[ignore = "requires the local Postman media simulation and FLOW2API_MEDIA_SIM_URL"]
    async fn media_transport_simulation() -> Result<()> {
        let base = std::env::var("FLOW2API_MEDIA_SIM_URL")
            .context("Start postman/simulations/desktop-media first")?;
        let directory = tempfile::tempdir()?;
        let app = App::open(directory.path())?;
        let settings = app.store.settings()?;
        for path in ["image/error", "image/html"] {
            assert!(app
                .archive(
                    &format!("{base}/{path}"),
                    "image",
                    "job",
                    "media",
                    &settings
                )
                .await
                .is_err());
            assert!(app.store.media()?.is_empty());
        }
        let media = app
            .archive(
                &format!("{base}/image/success"),
                "image",
                "job",
                "media",
                &settings,
            )
            .await?;
        assert!(media.filename.ends_with(".png"));
        let bytes =
            tokio::fs::read(directory.path().join("generated").join(&media.filename)).await?;
        assert_eq!(media.filename, format!("{:x}.png", Sha256::digest(&bytes)));
        assert_eq!(app.store.media()?.len(), 1);
        assert!(directory
            .path()
            .join("generated")
            .join(&media.filename)
            .with_extension("json")
            .is_file());
        Ok(())
    }
}
