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
        bail!(
            "当前用途需要 {}–{} 张参考图，已提供 {} 张；请调整参考图或切换用途",
            capabilities["min_images"],
            capabilities["max_images"],
            request.images.len()
        );
    }
    if capabilities["requires_video_id"] == true
        && request
            .video_id
            .as_deref()
            .unwrap_or_default()
            .trim()
            .is_empty()
    {
        bail!("请填写续写所需的源视频 ID");
    }
    Ok(model)
}

impl App {
    pub async fn media_reference(&self, filename: &str) -> Result<String> {
        let media = self
            .store
            .media()?
            .into_iter()
            .find(|m| m.filename == filename)
            .context("作品不存在，请刷新作品库")?;
        if media.kind != "image"
            || std::path::Path::new(filename)
                .file_name()
                .and_then(|s| s.to_str())
                != Some(filename)
        {
            bail!("请选择本机图片作品作为参考图");
        }
        let root = self.store.root.join("generated").canonicalize()?;
        let path = root.join(filename).canonicalize()?;
        if !path.starts_with(&root) || tokio::fs::metadata(&path).await?.len() > 20 * 1024 * 1024 {
            bail!("参考图无法读取或超过 20 MB，请选择较小的图片");
        }
        let bytes = tokio::fs::read(path).await?;
        if bytes.len() > 20 * 1024 * 1024 {
            bail!("参考图超过 20 MB");
        }
        let mime = match media_extension(&bytes, "image")? {
            "jpg" => "image/jpeg",
            "png" => "image/png",
            "webp" => "image/webp",
            _ => "image/gif",
        };
        Ok(format!("data:{mime};base64,{}", STANDARD.encode(bytes)))
    }
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
        account.last_synced = None;
        self.store.save_account(&account)?;
        self.refresh()?;
        let session = self.account_session(&account).await?;
        let mut session = session.lock().await;
        self.verify_login(&mut session, &mut account).await?;
        let credits = flow::rpc(&mut session, "nzlxg", json!([]), None).await?;
        account.credits = Some(flow::credits(&credits)?);
        account.last_synced = Some(now());
        account.last_error = None;
        account.consecutive_failures = 0;
        self.store.save_account(&account)?;
        if account.browser_bridge {
            if let Some(peer) = self.bridge.get(id) {
                peer.set_state("ready", "身份与余额校验通过，可以创作");
            }
        }
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
            ..Account::default()
        };
        if let Ok(credits) = flow::rpc(&mut session, "nzlxg", json!([]), None)
            .await
            .and_then(|v| flow::credits(&v))
        {
            account.credits = Some(credits);
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
        account.credits = Some(flow::credits(&credits)?);
        account.last_synced = Some(now());
        account.last_error = None;
        account.consecutive_failures = 0;
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
        request.prompt = request.prompt.trim().to_owned();
        request.video_id = request
            .video_id
            .map(|id| id.trim().to_owned())
            .filter(|id| !id.is_empty());
        let model = validate(&request)?;
        let is_video = model["config"]["type"] == "video";
        if self.status().queued >= 128 {
            bail!("排队任务过多，请稍后提交");
        }
        if request.account_id.is_none() {
            if let Some(video_id) = request.video_id.as_deref() {
                if let Ok(items) = self.store.media() {
                    if let Some(source) = items.into_iter().find(|m| m.media_id == video_id) {
                        if let Ok(source_task) = self.store.task(&source.task_id) {
                            request.account_id = source_task.request.account_id;
                        }
                    }
                }
            }
        }
        let accounts = self.store.accounts()?;
        let is_ready = |a: &Account| {
            a.available()
                && a.supports_kind(is_video)
                && a.last_synced.is_some()
                && (!a.browser_bridge || self.bridge.ready(&a.id))
        };
        let account = match request.account_id.as_deref() {
            Some(id) => {
                let candidate = accounts
                    .into_iter()
                    .find(|a| a.id == id)
                    .context("所选账号不存在")?;
                if !candidate.enabled {
                    bail!(
                        "所选账号 {} 已停用，请先启用或改用自动分配",
                        candidate.email
                    );
                }
                if candidate.temporarily_disabled() {
                    let reason = candidate
                        .disabled_reason
                        .as_deref()
                        .filter(|s| !s.is_empty())
                        .map(|r| format!("（{r}）"))
                        .unwrap_or_default();
                    bail!(
                        "所选账号 {} 正在临时禁用冷却中{reason}，请在账号页解除临时禁用或改用其他账号",
                        candidate.email
                    );
                }
                if !candidate.supports_kind(is_video) {
                    bail!(
                        "所选账号 {} 已关闭{}生成开关，请在账号页开启或改用其他账号",
                        candidate.email,
                        if is_video { "视频" } else { "图片" }
                    );
                }
                if !is_ready(&candidate) {
                    bail!(
                        "所选账号 {} 尚未完成登录同步或浏览器连接未就绪",
                        candidate.email
                    );
                }
                candidate
            }
            None => {
                let tasks = self.store.task_summaries()?;
                let has_temp_disabled = accounts
                    .iter()
                    .any(|a| a.enabled && a.temporarily_disabled());
                let has_kind_disabled = accounts
                    .iter()
                    .any(|a| a.available() && !a.supports_kind(is_video));
                accounts
                    .into_iter()
                    .filter(is_ready)
                    .min_by_key(|a| account_load(a, &tasks))
                    .ok_or_else(|| {
                        if has_temp_disabled {
                            anyhow::anyhow!(
                                "没有可用账号（部分账号处于临时禁用冷却中，可在账号页解除或等待恢复）"
                            )
                        } else if has_kind_disabled {
                            anyhow::anyhow!(
                                "没有开启{}生成且完成同步的可用账号，请在账号页检查能力开关",
                                if is_video { "视频" } else { "图片" }
                            )
                        } else {
                            anyhow::anyhow!("没有已启用且完成登录同步的账号")
                        }
                    })?
            }
        };
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
                let raw = error.to_string();
                let mut account_email = None;
                if let Ok(task) = app.store.task(&id) {
                    if let Some(account_id) = task.request.account_id.as_deref() {
                        if let Ok(mut account) = app.store.account(account_id) {
                            account_email = Some(account.email.clone());
                            account.last_error = Some(raw.clone());
                            account.consecutive_failures =
                                account.consecutive_failures.saturating_add(1);
                            let _ = app.store.save_account(&account);
                        }
                    }
                }
                let detailed = match account_email {
                    Some(email) if !raw.contains(&email) => format!("账号 {email}：{raw}"),
                    _ => raw,
                };
                if let Ok(mut task) = app.store.task(&id) {
                    task.stage = summarize_failure_stage(&task.stage, &detailed);
                    task.state = "failed".into();
                    task.error = Some(detailed.clone());
                    task.updated_at = now();
                    let _ = app.store.save_task(&task);
                }
                app.change_status(|s| s.last_error = Some(detailed));
            }
            let _ = app.refresh();
        });
        Ok(task)
    }
    pub async fn cancel_task(&self, id: &str) -> Result<Task> {
        let _admission = self.job_gate.lock().await;
        self.ensure_available()?;
        let mut task = self.store.task(id)?;
        if task.state == "cancelled" {
            return Ok(task);
        }
        if task.state != "queued" {
            bail!("只能取消排队任务；已经执行的任务请等待结果，避免重复扣费");
        }
        task.state = "cancelled".into();
        task.stage = "已取消，未提交到 Flow".into();
        task.error = Some("任务已在本机取消，未提交到 Flow".into());
        task.updated_at = now();
        self.store.save_task(&task)?;
        self.refresh()?;
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
        let admission = self.job_gate.lock().await;
        let mut task = self.store.task(id)?;
        if task.state != "queued" {
            return Ok(());
        }
        task.state = "running".into();
        task.stage = "等待账号浏览器就绪".into();
        task.updated_at = now();
        self.store.save_task(&task)?;
        self.refresh()?;
        drop(admission);
        let settings = self.store.settings()?;
        let model = validate(&task.request)?;
        let timeout_seconds = if model["config"]["type"] == "image" {
            settings.image_timeout_seconds
        } else {
            settings.video_timeout_seconds
        };
        let account = self
            .store
            .account(task.request.account_id.as_deref().context("任务缺少账号")?)?;
        let session = self.account_session(&account).await?;
        // Sessions serialize requests for one Google account. Waiting for another
        // task's browser must not consume this task's generation deadline.
        let mut session = session.lock().await;
        // A previous task may have created the project while this one waited.
        let mut account = self.store.account(&account.id)?;
        let media = timeout(
            Duration::from_secs(timeout_seconds),
            self.generate(&task, &model, &settings, &mut session, &mut account),
        )
        .await
        .context("生成超时，Flow 可能仍在执行；请核对上游结果后决定是否重试")??;
        if account.last_error.is_some() || account.consecutive_failures > 0 {
            account.last_error = None;
            account.consecutive_failures = 0;
            let _ = self.store.save_account(&account);
        }
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
        session: &mut Session,
        account: &mut Account,
    ) -> Result<Vec<Media>> {
        self.verify_login(session, account).await?;
        if account.project_id.is_empty() {
            self.stage(&task.id, "创建 Flow 项目")?;
            let payload = flow::rpc(
                session,
                "jHPbke",
                json!(["projects/*", [null, ["Flow2API Desktop"]], [null, 22]]),
                None,
            )
            .await?;
            account.project_id = flow::find_uuid(&payload, &[]).context("创建项目未返回项目 ID")?;
            self.store.save_account(account)?;
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
            let payload = flow::rpc(session, "maseQ", argument, Some(&account.project_id)).await?;
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
            let payload = flow::rpc(session, "ogiZ0b", argument, Some(&account.project_id)).await?;
            let items = payload[0].as_array().context("图片生成未返回媒体")?;
            let mut results = vec![];
            for item in items {
                let mut url = flow::media_url(item, "image").context("图片生成未返回下载地址")?;
                let media_id = item[0].as_str().context("图片缺少媒体 ID")?;
                if let Some(upsample) = cfg["upsample"].as_str() {
                    self.stage(&task.id, "图片高清放大")?;
                    let captcha = session.captcha("IMAGE_GENERATION").await?;
                    let payload = flow::rpc(
                        session,
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
            let payload = match flow::rpc(session, rpc, argument, Some(&account.project_id)).await {
                Ok(payload) => payload,
                Err(error) if key.contains("_relaxed") && is_pre_submission_model_error(&error) => {
                    let fallback_key = key.replace("_relaxed", "");
                    let captcha = session.captcha("VIDEO_GENERATION").await?;
                    let (rpc, argument) = flow::video_argument(
                        &account.project_id,
                        &task.request.prompt,
                        &fallback_key,
                        aspect,
                        &captcha,
                        mode,
                        &references,
                        task.request.video_id.as_deref(),
                        None,
                    )?;
                    flow::rpc(session, rpc, argument, Some(&account.project_id)).await?
                }
                Err(error) => return Err(error),
            };
            let videos = self
                .poll_video(session, task, &account.project_id, payload)
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
                        flow::rpc(session, rpc, argument, Some(&account.project_id)).await?;
                    self.poll_video(session, task, &account.project_id, payload)
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

fn is_pre_submission_model_error(error: &anyhow::Error) -> bool {
    let text = error.to_string();
    text.contains("Flow RPC 返回错误或缺少数据")
        || text.contains("PUBLIC_ERROR_MODEL_ACCESS_DENIED")
        || text.contains("ERROR_MODEL_ACCESS_DENIED")
        || text.contains("NOT_FOUND")
        || text.contains("INVALID_ARGUMENT")
}

pub fn summarize_failure_stage(previous_stage: &str, error_text: &str) -> String {
    let step = previous_stage.trim();
    let reason = if error_text.contains("MODEL_ACCESS_DENIED")
        || error_text.contains("无该模型权限")
        || error_text.contains("无此模型")
    {
        "账号无该模型权限"
    } else if error_text.contains("UNSAFE_GENERATION") || error_text.contains("安全审核") {
        "触发安全审核拦截"
    } else if error_text.contains("PROMINENT_PEOPLE") || error_text.contains("人物保护") {
        "触发人物保护拦截"
    } else if error_text.contains("QUOTA_EXCEEDED")
        || error_text.contains("INSUFFICIENT_CREDITS")
        || error_text.contains("积分不足")
        || error_text.contains("RESOURCE_EXHAUSTED")
    {
        "账号积分不足或超额"
    } else if error_text.contains("RATE_LIMIT")
        || error_text.contains("TOO_MANY_REQUESTS")
        || error_text.contains("限流")
    {
        "请求触发上游限流"
    } else if error_text.contains("NOT_FOUND") || error_text.contains("缺少数据") {
        "上游拒绝或不支持该模型"
    } else if error_text.contains("未连接") || error_text.contains("已断开") {
        "浏览器连接已断开"
    } else if error_text.contains("不一致")
        || error_text.contains("重新登录")
        || error_text.contains("UNAUTHENTICATED")
    {
        "账号需重新登录同步"
    } else if error_text.contains("验证码") {
        "获取验证码失败"
    } else if error_text.contains("超时") {
        "等待响应超时"
    } else {
        "执行失败"
    };
    if step.is_empty()
        || step == "等待执行"
        || step.starts_with("执行失败")
        || step.starts_with("失败")
    {
        format!("失败：{reason}")
    } else {
        format!("失败（{step}）：{reason}")
    }
}

fn account_load(
    account: &Account,
    tasks: &[Task],
) -> (
    usize,
    std::cmp::Reverse<i32>,
    u32,
    String,
    std::cmp::Reverse<i64>,
) {
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
    (
        busy,
        std::cmp::Reverse(account.priority),
        account.consecutive_failures,
        last_used,
        std::cmp::Reverse(account.credits.unwrap_or(0)),
    )
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
    fn test_task(id: &str, state: &str) -> Task {
        Task {
            id: id.into(),
            request: GenerationRequest {
                model: "gemini-3.1-flash-lite-image-square".into(),
                prompt: "test".into(),
                images: vec!["data:image/png;base64,large-reference".into()],
                account_id: None,
                video_id: None,
            },
            state: state.into(),
            stage: "waiting".into(),
            created_at: now(),
            updated_at: now(),
            error: None,
            results: vec![],
        }
    }
    #[tokio::test]
    async fn cancelled_queue_never_runs_and_detail_retains_references() -> Result<()> {
        let directory = tempfile::tempdir()?;
        let app = App::open(directory.path())?;
        let task = test_task("queued", "queued");
        app.store.save_task(&task)?;
        app.refresh()?;
        assert_eq!(app.status().queued, 1);
        assert!(app.store.task_summaries()?[0].request.images.is_empty());
        assert_eq!(
            app.store.task("queued")?.request.images,
            task.request.images
        );
        assert!(app.store.task("missing").is_err());
        assert_eq!(app.cancel_task("queued").await?.state, "cancelled");
        app.run_task("queued").await?;
        assert_eq!(app.store.task("queued")?.state, "cancelled");
        assert_eq!(app.status().queued, 0);
        assert_eq!(app.status().failed, 0);
        assert_eq!(app.cancel_task("queued").await?.state, "cancelled");
        for state in ["running", "succeeded", "failed", "interrupted"] {
            app.store.save_task(&test_task(state, state))?;
            assert!(app.cancel_task(state).await.is_err());
            assert_eq!(app.store.task(state)?.state, state);
        }
        app.store.save_task(&test_task("race", "queued"))?;
        let (cancel, run) = tokio::join!(app.cancel_task("race"), app.run_task("race"));
        // Cancellation takes the shared admission lock first; no browser is opened.
        assert!(cancel.is_ok());
        run?;
        assert_eq!(app.store.task("race")?.state, "cancelled");
        Ok(())
    }
    #[tokio::test]
    async fn local_reference_reads_only_registered_images() -> Result<()> {
        let directory = tempfile::tempdir()?;
        let app = App::open(directory.path())?;
        app.store.save_task(&test_task("source", "succeeded"))?;
        let media = app
            .archive_bytes(b"\x89PNG\r\n\x1a\nfixture", "image", "source", "media-id")
            .await?;
        let reference = app.media_reference(&media.filename).await?;
        assert!(reference.starts_with("data:image/png;base64,"));
        assert!(app.media_reference("../desktop.db").await.is_err());
        assert!(app.media_reference("unknown.png").await.is_err());
        Ok(())
    }
    #[test]
    fn accounts_prefer_free_then_priority_then_least_recently_used() {
        let account = |id: &str| Account {
            id: id.into(),
            email: format!("{id}@example.test"),
            enabled: true,
            last_synced: Some(now()),
            ..Account::default()
        };
        let first = account("first");
        let second = account("second");
        let mut high = account("high");
        high.priority = 100;
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
        assert!(account_load(&high, &[task.clone()]) < account_load(&second, &[task.clone()]));
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
