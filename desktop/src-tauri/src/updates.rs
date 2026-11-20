//! Updates from the public releases repository. No GitHub login is needed:
//! metadata and assets are read anonymously and every package is verified
//! against the updater public key before it is installed.

use serde::Serialize;
use serde_json::Value;
use std::path::{Path, PathBuf};
use std::sync::atomic::Ordering;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use tauri::{Emitter, Manager};
use tauri_plugin_updater::UpdaterExt;

const OWNER_REPO: &str = "melotusme/flow2api-releases";
const LIMIT: usize = 256 * 1024 * 1024;
const CHECK_INTERVAL: Duration = Duration::from_secs(2 * 3600);
const RETRY_INTERVAL: Duration = Duration::from_secs(15 * 60);
const RATE_LIMIT_INTERVAL: Duration = Duration::from_secs(30 * 60);
const BACKGROUND_RESTART_FILE: &str = "background-restart";
const BACKGROUND_RESTART_MAX_AGE_SECS: u64 = 300;

struct StagedUpdate {
    update: tauri_plugin_updater::Update,
    cache_path: Option<PathBuf>,
}

#[derive(Default)]
pub struct State {
    op: tokio::sync::Mutex<()>,
    pending: tokio::sync::Mutex<Option<StagedUpdate>>,
    status: std::sync::Mutex<UpdateStatus>,
    wake: tokio::sync::Notify,
}

#[derive(Clone, Default, Serialize)]
pub struct UpdateStatus {
    pub phase: String,
    pub message: String,
    pub checked_at: Option<u64>,
    pub current_version: String,
    pub available_version: Option<String>,
    pub notes: Option<String>,
    pub downloaded: bool,
    pub auto_update: bool,
    pub received: Option<usize>,
    pub total: Option<u64>,
}

#[derive(Serialize)]
pub struct Available {
    version: String,
    current_version: String,
    body: Option<String>,
    downloaded: bool,
}

fn unix_now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

pub fn consume_background_restart(root: &Path) -> bool {
    let path = root.join(BACKGROUND_RESTART_FILE);
    let Ok(raw) = std::fs::read_to_string(&path) else {
        return false;
    };
    let _ = std::fs::remove_file(&path);
    raw.trim()
        .parse::<u64>()
        .is_ok_and(|ts| unix_now().saturating_sub(ts) <= BACKGROUND_RESTART_MAX_AGE_SECS)
}

fn write_background_restart(root: &Path) {
    let _ = std::fs::write(root.join(BACKGROUND_RESTART_FILE), unix_now().to_string());
}

fn clear_background_restart(root: &Path) {
    let _ = std::fs::remove_file(root.join(BACKGROUND_RESTART_FILE));
}

fn current_auto_update(app: &tauri::AppHandle) -> bool {
    app.state::<super::Desktop>()
        .core
        .store
        .settings()
        .map(|s| s.auto_update)
        .unwrap_or(true)
}

fn current_proxy_url(app: &tauri::AppHandle) -> String {
    app.state::<super::Desktop>()
        .core
        .store
        .settings()
        .map(|s| s.proxy_url)
        .unwrap_or_default()
}

fn mutate_status(app: &tauri::AppHandle, update: impl FnOnce(&mut UpdateStatus)) {
    let state = app.state::<State>();
    let snapshot = {
        let mut status = state.status.lock().unwrap();
        if status.current_version.is_empty() {
            status.current_version = app.package_info().version.to_string();
        }
        status.auto_update = current_auto_update(app);
        update(&mut status);
        status.clone()
    };
    super::sync_update_tray(app, &snapshot);
    let _ = app.emit("update-status", snapshot);
}

fn report(app: &tauri::AppHandle, phase: &str, message: impl Into<String>) {
    let message = message.into();
    mutate_status(app, |status| {
        status.phase = phase.into();
        status.message = message;
        if matches!(phase, "available" | "current" | "ready") {
            status.checked_at = Some(unix_now());
        }
        if phase != "downloading" {
            status.received = None;
            status.total = None;
        }
    });
}

#[tauri::command]
pub fn update_status(app: tauri::AppHandle, state: tauri::State<'_, State>) -> UpdateStatus {
    let mut status = state.status.lock().unwrap().clone();
    if status.current_version.is_empty() {
        status.current_version = app.package_info().version.to_string();
    }
    status.auto_update = current_auto_update(&app);
    if status.phase.is_empty() {
        status.phase = "idle".into();
    }
    status
}

#[tauri::command]
pub fn set_auto_update(
    app: tauri::AppHandle,
    state: tauri::State<'_, State>,
    enabled: bool,
) -> Result<bool, String> {
    let desktop = app.state::<super::Desktop>();
    let actual = desktop
        .core
        .set_auto_update(enabled)
        .map_err(|e| e.to_string())?;
    mutate_status(&app, |status| {
        status.auto_update = actual;
    });
    if actual {
        state.wake.notify_one();
    }
    Ok(actual)
}

pub fn wake(app: &tauri::AppHandle) {
    app.state::<State>().wake.notify_one();
}

fn http(proxy_url: &str, download: bool) -> Result<reqwest::Client, String> {
    // Follow GitHub's download redirect only to its own release CDN.
    let policy = reqwest::redirect::Policy::custom(|attempt| {
        if attempt.previous().len() >= 3 || !valid_cdn_url(attempt.url()) {
            attempt.stop()
        } else {
            attempt.follow()
        }
    });
    let mut builder = reqwest::Client::builder()
        .redirect(policy)
        .user_agent("Flow2API-updater")
        .connect_timeout(Duration::from_secs(15))
        .read_timeout(Duration::from_secs(45))
        .timeout(Duration::from_secs(if download { 600 } else { 30 }));
    let proxy = proxy_url.trim();
    if !proxy.is_empty() {
        let parsed = reqwest::Proxy::all(proxy)
            .map_err(|_| "更新连接代理配置无效，请检查设置中的代理地址。".to_string())?;
        builder = builder.proxy(parsed);
    }
    builder.build().map_err(|_| "无法初始化更新连接。".into())
}

pub(crate) fn asset_name(platform: &str) -> Result<&'static str, String> {
    match platform {
        "windows-x86_64" => Ok("Flow2API.windows.x64.exe"),
        "linux-x86_64" => Ok("Flow2API.linux.x64.AppImage"),
        "darwin-aarch64" | "darwin-x86_64" => Ok("Flow2API.app.tar.gz"),
        _ => Err(format!("更新清单包含不支持的平台 {platform}。")),
    }
}

fn download_prefix() -> String {
    format!("https://github.com/{OWNER_REPO}/releases/download/")
}

fn asset(release: &Value, name: &str) -> Result<String, String> {
    let prefix = download_prefix();
    release["assets"]
        .as_array()
        .and_then(|assets| assets.iter().find(|asset| asset["name"] == name))
        .and_then(|asset| asset["browser_download_url"].as_str())
        .filter(|url| valid_download_url(url, &prefix))
        .map(str::to_string)
        .ok_or_else(|| format!("Release 缺少有效的 {name}，请检查发布是否完整。"))
}

fn valid_download_url(url: &str, prefix: &str) -> bool {
    url.strip_prefix(prefix).is_some_and(|rest| {
        let mut parts = rest.split('/');
        matches!((parts.next(), parts.next(), parts.next()), (Some(tag), Some(file), None)
            if !tag.is_empty() && !file.is_empty() && !rest.contains(['?', '#', '\\']))
    })
}

fn latest_url() -> String {
    format!("https://api.github.com/repos/{OWNER_REPO}/releases/latest")
}

async fn get_once(
    client: &reqwest::Client,
    url: &str,
    binary: bool,
) -> Result<reqwest::Response, (String, bool)> {
    if url != latest_url() && !valid_download_url(url, &download_prefix()) {
        return Err(("拒绝访问发布仓库以外的地址。".into(), false));
    }
    let response = client
        .get(url)
        .header(
            reqwest::header::ACCEPT,
            if binary {
                "application/octet-stream"
            } else {
                "application/vnd.github+json"
            },
        )
        .send()
        .await
        .map_err(|_| {
            (
                "GitHub 更新连接失败或超时，请检查网络或代理设置。".into(),
                true,
            )
        })?;
    match response.status().as_u16() {
        200..=299 => Ok(response),
        403 | 429 => Err(("GitHub 暂时限制了更新查询频率，请稍后重试。".into(), false)),
        404 => Err(("发布仓库还没有正式 Release。".into(), false)),
        code if response.status().is_redirection() => Err((
            format!("GitHub 返回了不受信任的下载跳转（HTTP {code}）。"),
            false,
        )),
        code @ 500..=599 => Err((
            format!("GitHub 更新服务暂时异常（HTTP {code}），请稍后重试。"),
            true,
        )),
        code => Err((
            format!("GitHub 更新请求失败（HTTP {code}），请稍后重试。"),
            false,
        )),
    }
}

async fn get(
    client: &reqwest::Client,
    url: &str,
    binary: bool,
) -> Result<reqwest::Response, String> {
    let mut last_error = String::new();
    for (attempt, delay_secs) in [0u64, 2, 5].into_iter().enumerate() {
        if attempt > 0 {
            tokio::time::sleep(Duration::from_secs(delay_secs)).await;
        }
        match get_once(client, url, binary).await {
            Ok(response) => return Ok(response),
            Err((error, retryable)) => {
                if !retryable {
                    return Err(error);
                }
                last_error = error;
            }
        }
    }
    Err(last_error)
}

fn valid_cdn_url(url: &reqwest::Url) -> bool {
    url.scheme() == "https"
        && url.username().is_empty()
        && url.password().is_none()
        && url.port_or_known_default() == Some(443)
        && matches!(
            url.host_str(),
            Some(
                "release-assets.githubusercontent.com"
                    | "objects.githubusercontent.com"
                    | "github-releases.githubusercontent.com"
            )
        )
}

fn cache_dir(root: &Path) -> PathBuf {
    root.join("updates")
}

fn cache_file(root: &Path, version: &str, download_url: &str) -> Result<PathBuf, String> {
    if version.is_empty()
        || !version
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'-' | b'+'))
    {
        return Err("更新版本号格式无效。".into());
    }
    let file = download_url
        .rsplit('/')
        .next()
        .filter(|name| {
            !name.is_empty() && !name.contains(['/', '\\']) && *name != "." && *name != ".."
        })
        .ok_or_else(|| "更新包文件名无效。".to_string())?;
    Ok(cache_dir(root).join(format!("v{version}-{file}")))
}

fn prune_cache(root: &Path, keep: Option<&Path>) {
    let dir = cache_dir(root);
    let Ok(entries) = std::fs::read_dir(&dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if keep.is_some_and(|kept| kept == path) {
            continue;
        }
        let _ = std::fs::remove_file(&path);
    }
}

fn verified_cached_bytes(update: &tauri_plugin_updater::Update, path: &Path) -> Option<Vec<u8>> {
    let meta = std::fs::metadata(path).ok()?;
    if !meta.is_file() || meta.len() > LIMIT as u64 {
        let _ = std::fs::remove_file(path);
        return None;
    }
    let bytes = std::fs::read(path).ok()?;
    if update.verify(&bytes).is_ok() {
        Some(bytes)
    } else {
        let _ = std::fs::remove_file(path);
        None
    }
}

#[tauri::command]
pub async fn check_update(
    app: tauri::AppHandle,
    state: tauri::State<'_, State>,
) -> Result<Option<Available>, String> {
    let _op = state.op.lock().await;
    let result = check_inner(&app, &state).await;
    if let Err(error) = &result {
        report(&app, "error", error);
    }
    result
}

async fn check_inner(app: &tauri::AppHandle, state: &State) -> Result<Option<Available>, String> {
    report(app, "checking", "正在检查更新…");
    let proxy = current_proxy_url(app);
    let client = http(&proxy, false)?;
    let release: Value = get(&client, &latest_url(), false)
        .await?
        .json()
        .await
        .map_err(|_| "Release 信息格式异常。")?;
    let mut manifest: Value = get(&client, &asset(&release, "latest.json")?, true)
        .await?
        .json()
        .await
        .map_err(|_| "更新清单格式异常。")?;
    if release["draft"] == true
        || release["prerelease"] == true
        || release["tag_name"].as_str()
            != manifest["version"]
                .as_str()
                .map(|v| format!("v{v}"))
                .as_deref()
    {
        return Err("正式 Release 与更新清单版本不一致。".into());
    }
    if let Some(platforms) = manifest["platforms"].as_object_mut() {
        for (key, platform) in platforms.iter_mut() {
            platform["url"] = Value::String(asset(&release, asset_name(key)?)?);
        }
    }
    let exit_app = app.clone();
    let update = app
        .updater_builder()
        .restart_after_install(true)
        .on_before_exit(move || {
            exit_app
                .state::<super::Desktop>()
                .core
                .store
                .release_instance_lock();
            tauri_plugin_single_instance::destroy(&exit_app);
        })
        .build()
        .map_err(|_| "更新器配置异常。")?
        .restore_update(manifest)
        .map_err(|_| "更新清单无效或不支持当前架构。")?;

    let root = app.state::<super::Desktop>().core.store.root.clone();
    let mut pending = state.pending.lock().await;
    let Some(update) = update else {
        *pending = None;
        prune_cache(&root, None);
        mutate_status(app, |status| {
            status.phase = "current".into();
            status.message = "已检查，当前是最新版本。".into();
            status.checked_at = Some(unix_now());
            status.available_version = None;
            status.notes = None;
            status.downloaded = false;
            status.received = None;
            status.total = None;
        });
        return Ok(None);
    };

    let target = cache_file(&root, &update.version, update.download_url.as_str())?;
    let cached = verified_cached_bytes(&update, &target).is_some();
    if cached {
        prune_cache(&root, Some(&target));
    }
    let available = Available {
        version: update.version.clone(),
        current_version: update.current_version.clone(),
        body: update.body.clone(),
        downloaded: cached,
    };
    *pending = Some(StagedUpdate {
        update,
        cache_path: cached.then_some(target),
    });
    mutate_status(app, |status| {
        status.phase = if cached { "ready" } else { "available" }.into();
        status.message = if cached {
            format!(
                "新版本 v{} 已在后台下载并验签，可随时安装重启。",
                available.version
            )
        } else if status.auto_update {
            format!(
                "发现新版本 v{}，将在后台下载并在任务空闲时自动安装。",
                available.version
            )
        } else {
            format!("发现新版本 v{}，点击即可下载并安装。", available.version)
        };
        status.checked_at = Some(unix_now());
        status.available_version = Some(available.version.clone());
        status.notes = available.body.clone();
        status.downloaded = cached;
        status.received = None;
        status.total = None;
    });
    Ok(Some(available))
}

async fn stage_pending(app: &tauri::AppHandle, state: &State) -> Result<PathBuf, String> {
    let (update, existing_path) = {
        let pending = state.pending.lock().await;
        let staged = pending.as_ref().ok_or("请先检查新版本。")?;
        (staged.update.clone(), staged.cache_path.clone())
    };
    let root = app.state::<super::Desktop>().core.store.root.clone();
    let target = existing_path.unwrap_or(cache_file(
        &root,
        &update.version,
        update.download_url.as_str(),
    )?);
    if verified_cached_bytes(&update, &target).is_some() {
        if let Some(staged) = state.pending.lock().await.as_mut() {
            if staged.update.version == update.version {
                staged.cache_path = Some(target.clone());
            }
        }
        mutate_status(app, |status| {
            status.phase = "ready".into();
            status.message = format!(
                "新版本 v{} 已在后台下载并验签，可随时安装重启。",
                update.version
            );
            status.downloaded = true;
            status.received = None;
            status.total = None;
        });
        return Ok(target);
    }

    report(
        app,
        "downloading",
        format!("正在后台下载 v{}…", update.version),
    );
    let proxy = current_proxy_url(app);
    let client = http(&proxy, true)?;
    let mut response = get(&client, update.download_url.as_str(), true).await?;
    let total = response.content_length();
    if total.is_some_and(|size| size > LIMIT as u64) {
        return Err("更新包超过 256 MiB 限制。".into());
    }
    let mut bytes = Vec::new();
    let mut last_emit = Instant::now() - Duration::from_secs(1);
    while let Some(chunk) = response
        .chunk()
        .await
        .map_err(|_| "更新包下载中断，将自动重试。")?
    {
        if bytes.len() + chunk.len() > LIMIT {
            return Err("更新包超过 256 MiB 限制。".into());
        }
        bytes.extend_from_slice(&chunk);
        if last_emit.elapsed() >= Duration::from_millis(150) {
            last_emit = Instant::now();
            let received = bytes.len();
            mutate_status(app, |status| {
                status.phase = "downloading".into();
                status.received = Some(received);
                status.total = total;
                status.message = match total {
                    Some(total) if total > 0 => format!(
                        "正在下载 v{}（{:.1} / {:.1} MB，{}%）…",
                        update.version,
                        received as f64 / 1_048_576.0,
                        total as f64 / 1_048_576.0,
                        ((received as u128 * 100) / total as u128).min(100)
                    ),
                    _ => format!(
                        "正在下载 v{}（{:.1} MB）…",
                        update.version,
                        received as f64 / 1_048_576.0
                    ),
                };
            });
            let _ = app.emit(
                "update-progress",
                serde_json::json!({"received": received, "total": total}),
            );
        }
    }
    let _ = app.emit(
        "update-progress",
        serde_json::json!({"received": bytes.len(), "total": total}),
    );
    update
        .verify(&bytes)
        .map_err(|_| "更新包签名校验失败，当前应用继续运行。")?;

    let dir = cache_dir(&root);
    tokio::fs::create_dir_all(&dir)
        .await
        .map_err(|_| "无法创建更新缓存目录。")?;
    let tmp = target.with_extension("tmp");
    tokio::fs::write(&tmp, &bytes)
        .await
        .map_err(|_| "无法写入更新缓存文件，请检查磁盘空间。")?;
    #[cfg(windows)]
    let _ = tokio::fs::remove_file(&target).await;
    tokio::fs::rename(&tmp, &target)
        .await
        .map_err(|_| "无法保存已验签的更新包。")?;
    prune_cache(&root, Some(&target));

    if let Some(staged) = state.pending.lock().await.as_mut() {
        if staged.update.version == update.version {
            staged.cache_path = Some(target.clone());
        }
    }
    mutate_status(app, |status| {
        status.phase = "ready".into();
        status.message = format!(
            "新版本 v{} 已下载并验证签名，将在空闲时安装重启。",
            update.version
        );
        status.downloaded = true;
        status.received = None;
        status.total = None;
    });
    Ok(target)
}

#[tauri::command]
pub async fn install_update(
    app: tauri::AppHandle,
    state: tauri::State<'_, State>,
) -> Result<(), String> {
    let _op = state.op.lock().await;
    let result = install_inner(&app, &state, false).await.map(|_| ());
    if let Err(error) = &result {
        report(&app, "error", error);
    }
    result
}

/// Returns Ok(true) if restarted/installed, Ok(false) if deferred because tasks are busy (auto mode).
async fn install_inner(
    app: &tauri::AppHandle,
    state: &State,
    automatic: bool,
) -> Result<bool, String> {
    let desktop = app.state::<super::Desktop>();
    if !automatic && desktop.core.status().busy() {
        return Err("仍有执行或排队任务，请等待任务完成后再安装更新。".into());
    }
    let cache_path = stage_pending(app, state).await?;
    let update = {
        let pending = state.pending.lock().await;
        pending.as_ref().ok_or("请先检查新版本。")?.update.clone()
    };
    if automatic
        && !desktop
            .core
            .store
            .settings()
            .map_err(|e| e.to_string())?
            .auto_update
    {
        return Err("本机自动更新已关闭，取消自动安装。".into());
    }
    if desktop.core.status().busy() {
        if automatic {
            report(
                app,
                "waiting",
                format!(
                    "新版本 v{} 已在后台下载并验签，将在当前任务完成后自动安装并重启。",
                    update.version
                ),
            );
            return Ok(false);
        }
        return Err("仍有执行或排队任务，请等待任务完成后再安装更新。".into());
    }
    let bytes = tokio::fs::read(&cache_path)
        .await
        .map_err(|_| "无法读取已下载的更新包，将重新下载。")?;
    update
        .verify(&bytes)
        .map_err(|_| "更新包签名校验失败，当前应用继续运行。")?;

    let was_running = desktop.core.status().service == "running";
    // shutdown holds task admission and rejects queued/running work atomically.
    if let Err(error) = desktop.core.shutdown().await {
        if automatic && desktop.core.status().busy() {
            report(
                app,
                "waiting",
                format!(
                    "新版本 v{} 已就绪，有新任务执行中，完成后将自动安装并重启。",
                    update.version
                ),
            );
            return Ok(false);
        }
        return Err(error.to_string());
    }

    let stay_in_background = automatic
        && !app
            .get_webview_window("main")
            .and_then(|window| window.is_visible().ok())
            .unwrap_or(false);
    if stay_in_background {
        write_background_restart(&desktop.core.store.root);
    } else {
        clear_background_restart(&desktop.core.store.root);
    }

    desktop.quitting.store(true, Ordering::SeqCst);
    report(
        app,
        "installing",
        format!("签名验证通过，正在安装 v{} 并重启…", update.version),
    );
    let _ = app.emit("update-installing", ());

    // Release the data-directory lock before installing so Windows NSIS or macOS LaunchServices
    // relaunch never races with this exiting process on `instance.lock`.
    desktop.core.store.release_instance_lock();
    let installed = tauri::async_runtime::spawn_blocking(move || update.install(&bytes)).await;
    if !matches!(installed, Ok(Ok(()))) {
        clear_background_restart(&desktop.core.store.root);
        let _ = desktop.core.store.reacquire_instance_lock();
        desktop.quitting.store(false, Ordering::SeqCst);
        desktop
            .core
            .resume_after_failed_update(was_running)
            .await
            .map_err(|_| "更新安装失败，API 恢复失败；请重新打开应用。")?;
        return Err("更新安装失败，已恢复当前版本运行，请重试。".into());
    }

    *state.pending.lock().await = None;
    let _ = tokio::fs::remove_file(&cache_path).await;
    tauri_plugin_single_instance::destroy(app);
    app.restart();
}

/// Native background scheduler: runs even when the window is hidden in the menu bar / tray.
/// Automatically checks, pre-downloads & verifies in the background, and silently installs
/// once all generation tasks finish.
pub fn start(app: tauri::AppHandle) {
    tauri::async_runtime::spawn(async move {
        let state = app.state::<State>();
        let desktop = app.state::<super::Desktop>();
        let mut status_rx = desktop.core.subscribe();

        // Give the app a short startup settle window before the first background check.
        tokio::select! {
            _ = tokio::time::sleep(Duration::from_secs(15)) => {}
            _ = state.wake.notified() => {}
        }

        let mut next_check = tokio::time::Instant::now();
        let mut next_action = tokio::time::Instant::now();

        loop {
            let enabled = current_auto_update(&app);
            let stopping = desktop.quit_in_progress.load(Ordering::SeqCst)
                || desktop.quitting.load(Ordering::SeqCst);

            if enabled && !stopping {
                if tokio::time::Instant::now() >= next_check {
                    let _op = state.op.lock().await;
                    let result = check_inner(&app, &state).await;
                    next_check = tokio::time::Instant::now()
                        + match &result {
                            Ok(_) => CHECK_INTERVAL,
                            Err(err) if err.contains("频率") => RATE_LIMIT_INTERVAL,
                            Err(_) => RETRY_INTERVAL,
                        };
                    if let Err(error) = result {
                        report(&app, "error", error);
                    }
                }

                let (has_pending, is_downloaded, version) = {
                    let pending = state.pending.lock().await;
                    match pending.as_ref() {
                        Some(staged) => (
                            true,
                            staged.cache_path.is_some(),
                            Some(staged.update.version.clone()),
                        ),
                        None => (false, false, None),
                    }
                };

                if has_pending && tokio::time::Instant::now() >= next_action {
                    // Stage (download + verify) immediately in the background, even while tasks run.
                    if !is_downloaded {
                        let _op = state.op.lock().await;
                        if let Err(error) = stage_pending(&app, &state).await {
                            report(&app, "error", error);
                            next_action =
                                tokio::time::Instant::now() + Duration::from_secs(10 * 60);
                            continue;
                        }
                    }

                    if desktop.core.status().busy() {
                        if let Some(ver) = version {
                            report(
                                &app,
                                "waiting",
                                format!("新版本 v{ver} 已在后台下载并验签，将在当前任务完成后自动安装并重启。"),
                            );
                        }
                    } else {
                        // Brief quiet settle window so back-to-back API scripts aren't interrupted mid-batch.
                        tokio::select! {
                            _ = tokio::time::sleep(Duration::from_secs(5)) => {}
                            _ = status_rx.changed() => {
                                if desktop.core.status().busy() {
                                    continue;
                                }
                            }
                        }
                        if current_auto_update(&app)
                            && !desktop.core.status().busy()
                            && !desktop.quit_in_progress.load(Ordering::SeqCst)
                            && !desktop.quitting.load(Ordering::SeqCst)
                        {
                            let _op = state.op.lock().await;
                            match install_inner(&app, &state, true).await {
                                Ok(true) => {}
                                Ok(false) => {}
                                Err(error) => {
                                    report(&app, "error", error);
                                    next_action =
                                        tokio::time::Instant::now() + Duration::from_secs(15 * 60);
                                }
                            }
                        }
                    }
                }
            }

            let has_staged = state.pending.lock().await.is_some();
            let now = tokio::time::Instant::now();
            let sleep_until = if enabled && has_staged {
                next_action.max(now + Duration::from_secs(30))
            } else {
                next_check.min(now + Duration::from_secs(300))
            };
            let wait = sleep_until.saturating_duration_since(now);

            tokio::select! {
                _ = state.wake.notified() => {
                    next_action = tokio::time::Instant::now();
                    if state.pending.lock().await.is_none() {
                        next_check = tokio::time::Instant::now();
                    }
                }
                changed = status_rx.changed(), if enabled && has_staged => {
                    if changed.is_ok() && !desktop.core.status().busy() {
                        next_action = tokio::time::Instant::now();
                    }
                }
                _ = tokio::time::sleep(wait) => {}
            }
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn assets_only_come_from_the_fixed_repository() {
        let ok = format!("{}v0.3.3/latest.json", download_prefix());
        let release =
            serde_json::json!({"assets":[{"name":"latest.json","browser_download_url":ok}]});
        assert!(asset(&release, "latest.json").is_ok());
        for url in [
            "https://evil.example/latest.json",
            "https://github.com/other/repo/releases/download/v1/latest.json",
            "https://github.com/melotusme/flow2api-releases/releases/download/v1/latest.json?x=1",
            "https://github.com/melotusme/flow2api-releases/releases/download/v1/a/b",
            "https://github.com/melotusme/flow2api-releases/releases/download//latest.json",
        ] {
            let release =
                serde_json::json!({"assets":[{"name":"latest.json","browser_download_url":url}]});
            assert!(asset(&release, "latest.json").is_err(), "{url}");
        }
    }

    #[test]
    fn redirects_only_reach_github_release_cdn() {
        for url in [
            "https://release-assets.githubusercontent.com/a?sig=abc",
            "https://objects.githubusercontent.com/a",
        ] {
            assert!(valid_cdn_url(&reqwest::Url::parse(url).unwrap()));
        }
        for url in [
            "http://release-assets.githubusercontent.com/a",
            "https://release-assets.githubusercontent.com.evil.test/a",
            "https://user@objects.githubusercontent.com/a",
            "https://objects.githubusercontent.com:8443/a",
            "https://github.com/other/a",
        ] {
            assert!(!valid_cdn_url(&reqwest::Url::parse(url).unwrap()));
        }
    }

    #[test]
    fn platform_assets_do_not_cross() {
        assert_eq!(
            asset_name("windows-x86_64").unwrap(),
            "Flow2API.windows.x64.exe"
        );
        assert_eq!(asset_name("darwin-aarch64").unwrap(), "Flow2API.app.tar.gz");
        assert_eq!(asset_name("darwin-x86_64").unwrap(), "Flow2API.app.tar.gz");
        assert!(asset_name("windows-aarch64").is_err());
    }

    #[test]
    fn background_restart_marker_is_one_shot_and_bounded() {
        let dir = std::env::temp_dir().join(format!("flow2api-bg-restart-{}", std::process::id()));
        let _ = std::fs::create_dir_all(&dir);
        assert!(!consume_background_restart(&dir));
        write_background_restart(&dir);
        assert!(consume_background_restart(&dir));
        // Second read must return false (one-shot).
        assert!(!consume_background_restart(&dir));
        // Stale marker (> 5 min old) is rejected and deleted.
        std::fs::write(dir.join(BACKGROUND_RESTART_FILE), "1000").unwrap();
        assert!(!consume_background_restart(&dir));
        assert!(!dir.join(BACKGROUND_RESTART_FILE).exists());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn cache_file_rejects_traversal_and_http_accepts_proxy() {
        let root = Path::new("/tmp/flow2api-test");
        let path = cache_file(
            root,
            "0.3.8",
            "https://github.com/melotusme/flow2api-releases/releases/download/v0.3.8/Flow2API.app.tar.gz",
        )
        .unwrap();
        assert!(path.ends_with("updates/v0.3.8-Flow2API.app.tar.gz"));
        assert!(cache_file(root, "../0.3.8", "https://example.com/a").is_err());
        assert!(http("", false).is_ok());
        assert!(http("http://127.0.0.1:7890", true).is_ok());
        assert!(http("not a url", false).is_err());
    }

    #[test]
    #[ignore = "network: reads the live public release"]
    fn live_public_release_is_readable_without_login() {
        tauri::async_runtime::block_on(async {
            let client = http("", false).unwrap();
            let release: Value = get(&client, &latest_url(), false)
                .await
                .unwrap()
                .json()
                .await
                .unwrap();
            let manifest: Value = get(&client, &asset(&release, "latest.json").unwrap(), true)
                .await
                .unwrap()
                .json()
                .await
                .unwrap();
            assert!(manifest["platforms"]
                .as_object()
                .is_some_and(|p| !p.is_empty()));
        });
    }
}
