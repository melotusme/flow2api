mod updates;

use flow2api_core::{catalog, model::*, App};
use serde_json::Value;
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc, Mutex,
};
use tauri::{
    menu::{CheckMenuItem, Menu, MenuItem, PredefinedMenuItem},
    tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent},
    Emitter, Manager,
};
use tauri_plugin_autostart::ManagerExt as AutostartExt;
use tauri_plugin_dialog::DialogExt;
use tauri_plugin_opener::OpenerExt;

struct Desktop {
    core: Arc<App>,
    quitting: AtomicBool,
    quit_in_progress: AtomicBool,
    webdav: tokio::sync::Mutex<()>,
    autostart_item: Mutex<Option<CheckMenuItem<tauri::Wry>>>,
    auto_update_item: Mutex<Option<CheckMenuItem<tauri::Wry>>>,
    update_menu_item: Mutex<Option<MenuItem<tauri::Wry>>>,
}

pub(crate) fn sync_update_tray(app: &tauri::AppHandle, status: &updates::UpdateStatus) {
    let Some(desktop) = app.try_state::<Desktop>() else {
        return;
    };
    if let Ok(slot) = desktop.auto_update_item.lock() {
        if let Some(item) = slot.as_ref() {
            let _ = item.set_checked(status.auto_update);
        }
    }
    if let Ok(slot) = desktop.update_menu_item.lock() {
        if let Some(item) = slot.as_ref() {
            let text = match (status.phase.as_str(), status.available_version.as_deref()) {
                ("checking", _) => "正在检查更新…".to_string(),
                ("downloading", Some(v)) => format!("正在后台下载 v{v}…"),
                ("ready", Some(v)) => format!("安装新版本 v{v} 并重启…"),
                ("waiting", Some(v)) => format!("v{v} 已就绪（等待空闲安装）"),
                ("available", Some(v)) => format!("发现新版本 v{v}…"),
                ("installing", Some(v)) => format!("正在安装 v{v}…"),
                _ => "检查更新…".to_string(),
            };
            let _ = item.set_text(text);
        }
    };
}
fn result<T>(core: &App, value: anyhow::Result<T>) -> Result<T, String> {
    value.map_err(|error| {
        let message = error.to_string();
        core.change_status(|s| s.last_error = Some(message.clone()));
        message
    })
}
#[tauri::command]
fn status(state: tauri::State<Desktop>) -> Status {
    state.core.status()
}
#[tauri::command]
fn settings(state: tauri::State<Desktop>) -> Result<Settings, String> {
    result(&state.core, state.core.store.settings())
}
#[tauri::command]
async fn save_settings(
    app: tauri::AppHandle,
    state: tauri::State<'_, Desktop>,
    settings: Settings,
) -> Result<(), String> {
    let auto_update = settings.auto_update;
    result(&state.core, state.core.save_settings(settings).await)?;
    if let Ok(slot) = state.auto_update_item.lock() {
        if let Some(item) = slot.as_ref() {
            let _ = item.set_checked(auto_update);
        }
    }
    if auto_update {
        updates::wake(&app);
    }
    Ok(())
}
#[tauri::command]
fn magpie_status(
    state: tauri::State<Desktop>,
) -> Result<flow2api_core::magpie::Connection, String> {
    let path = flow2api_core::magpie::config_path().map_err(|e| e.to_string())?;
    flow2api_core::magpie::inspect(
        &path,
        &state.core.store.settings().map_err(|e| e.to_string())?,
    )
    .map_err(|e| e.to_string())
}
#[tauri::command]
async fn connect_magpie(
    state: tauri::State<'_, Desktop>,
) -> Result<flow2api_core::magpie::Connection, String> {
    result(&state.core, state.core.start_service().await)?;
    let _gate = state.core.job_gate.lock().await;
    let path = flow2api_core::magpie::config_path().map_err(|e| e.to_string())?;
    let settings = state.core.store.settings().map_err(|e| e.to_string())?;
    flow2api_core::magpie::connect(&path, &settings).map_err(|e| e.to_string())
}
#[tauri::command]
async fn start_service(state: tauri::State<'_, Desktop>) -> Result<Status, String> {
    result(&state.core, state.core.start_service().await)
}
#[tauri::command]
async fn stop_service(state: tauri::State<'_, Desktop>) -> Result<Status, String> {
    result(&state.core, state.core.stop_service().await)
}
#[tauri::command]
fn models() -> Value {
    catalog::public_models()
}
#[tauri::command]
fn accounts(state: tauri::State<Desktop>) -> Result<Vec<Account>, String> {
    result(&state.core, state.core.store.accounts())
}
#[tauri::command]
fn browser_connections(
    state: tauri::State<Desktop>,
) -> Vec<flow2api_core::bridge::ConnectionStatus> {
    state.core.bridge.statuses()
}
#[tauri::command]
fn music_connections(state: tauri::State<Desktop>) -> Vec<flow2api_core::bridge::ConnectionStatus> {
    state.core.music_bridge.statuses()
}
#[derive(serde::Serialize)]
struct InstallExtensionResult {
    dir: String,
    profile: Option<String>,
    opened_browser: bool,
    opened_folder: bool,
}

#[tauri::command]
fn list_chrome_profiles(
    state: tauri::State<Desktop>,
) -> Vec<flow2api_core::browser::ChromeProfileInfo> {
    let ext_dir = state.core.store.root.join("chrome-extension");
    let _ = flow2api_core::export_chrome_extension(&ext_dir);
    flow2api_core::browser::inspect_chrome_profiles(&ext_dir)
}

#[tauri::command]
fn open_url_in_chrome_profile(
    app: tauri::AppHandle,
    state: tauri::State<Desktop>,
    profile: Option<String>,
    email: Option<String>,
    target: Option<String>,
) -> Result<Option<String>, String> {
    let url = match target.as_deref() {
        Some("music") => "https://www.flowmusic.app/session",
        _ => "https://flow.google.com/projects",
    };
    let ext_dir = state.core.store.root.join("chrome-extension");
    let resolved_profile = profile
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .or_else(|| {
            email
                .as_deref()
                .and_then(|e| flow2api_core::browser::find_chrome_profile_for_email(&ext_dir, e))
                .map(|p| p.profile)
        })
        .or_else(|| {
            let mut enabled_accounts = state.core.store.accounts().unwrap_or_default();
            enabled_accounts.retain(|a| a.enabled);
            enabled_accounts.sort_by(|a, b| b.priority.cmp(&a.priority));
            enabled_accounts.into_iter().find_map(|a| {
                flow2api_core::browser::find_chrome_profile_for_email(&ext_dir, &a.email)
                    .map(|p| p.profile)
            })
        });
    let browser_exe = state
        .core
        .store
        .settings()
        .map(|s| s.browser_executable)
        .unwrap_or_default();
    let opened = flow2api_core::browser::open_in_chrome_profile(
        &browser_exe,
        resolved_profile.as_deref(),
        url,
    )
    .map_err(|e| e.to_string())?;
    if !opened {
        app.opener()
            .open_url(url, None::<&str>)
            .map_err(|e| e.to_string())?;
    }
    Ok(resolved_profile)
}

#[tauri::command]
fn install_chrome_extension_for_profile(
    app: tauri::AppHandle,
    state: tauri::State<Desktop>,
    profile: Option<String>,
) -> Result<InstallExtensionResult, String> {
    let dir = state.core.store.root.join("chrome-extension");
    let exported = result(&state.core, flow2api_core::export_chrome_extension(&dir))?;
    let clean_profile = profile
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty());
    let browser_exe = state
        .core
        .store
        .settings()
        .map(|s| s.browser_executable)
        .unwrap_or_default();
    let opened_browser = flow2api_core::browser::open_in_chrome_profile(
        &browser_exe,
        clean_profile.as_deref(),
        "chrome://extensions/",
    )
    .unwrap_or(false);
    let mut opened_folder = flow2api_core::browser::reveal_extension_folder(&exported);
    if !opened_folder {
        opened_folder = app
            .opener()
            .open_path(exported.to_string_lossy(), None::<&str>)
            .is_ok();
    }
    Ok(InstallExtensionResult {
        dir: exported.to_string_lossy().into_owned(),
        profile: clean_profile,
        opened_browser,
        opened_folder,
    })
}

#[tauri::command]
fn open_flow_page(app: tauri::AppHandle, state: tauri::State<Desktop>) -> Result<(), String> {
    let _ = open_url_in_chrome_profile(app, state, None, None, Some("flow".into()))?;
    Ok(())
}
#[tauri::command]
fn open_flow_music_page(app: tauri::AppHandle, state: tauri::State<Desktop>) -> Result<(), String> {
    let _ = open_url_in_chrome_profile(app, state, None, None, Some("music".into()))?;
    Ok(())
}
#[tauri::command]
async fn save_account(
    state: tauri::State<'_, Desktop>,
    email: String,
    id: Option<String>,
    enabled: bool,
    priority: Option<i32>,
    note: Option<String>,
    image_enabled: Option<bool>,
    video_enabled: Option<bool>,
    music_enabled: Option<bool>,
    disabled_until: Option<String>,
    disabled_reason: Option<String>,
    clear_error: Option<bool>,
    reset_project: Option<bool>,
) -> Result<Account, String> {
    let email = email.trim().to_string();
    let mut account = match id {
        Some(id) => result(&state.core, state.core.store.account(&id))?,
        None => Account {
            id: uuid::Uuid::new_v4().to_string(),
            email: email.clone(),
            enabled,
            ..Account::default()
        },
    };
    if account.email != email {
        if state.core.status().busy() {
            return Err("请等待任务完成后修改账号邮箱".into());
        }
        account.last_synced = None;
        account.credits = None;
    }
    account.email = email;
    account.enabled = enabled;
    if let Some(priority) = priority {
        account.priority = priority.clamp(-1000, 1000);
    }
    if let Some(note) = note {
        account.note = note.trim().chars().take(500).collect();
    }
    if let Some(image_enabled) = image_enabled {
        account.image_enabled = image_enabled;
    }
    if let Some(video_enabled) = video_enabled {
        account.video_enabled = video_enabled;
    }
    if let Some(music_enabled) = music_enabled {
        account.music_enabled = music_enabled;
    }
    if let Some(until) = disabled_until {
        let until = until.trim();
        account.disabled_until = if until.is_empty() {
            None
        } else {
            Some(until.to_string())
        };
    }
    if let Some(reason) = disabled_reason {
        let reason = reason.trim();
        account.disabled_reason = if reason.is_empty() {
            None
        } else {
            Some(reason.chars().take(200).collect())
        };
    }
    if account.disabled_until.is_none() {
        account.disabled_reason = None;
    }
    if clear_error == Some(true) {
        account.last_error = None;
        account.consecutive_failures = 0;
    }
    if reset_project == Some(true) {
        account.project_id.clear();
    }
    result(&state.core, state.core.store.save_account(&account))?;
    result(&state.core, state.core.refresh())?;
    Ok(account)
}
#[tauri::command]
async fn temp_disable_account(
    state: tauri::State<'_, Desktop>,
    id: String,
    minutes: i64,
    reason: Option<String>,
) -> Result<Account, String> {
    let mut account = result(&state.core, state.core.store.account(&id))?;
    if minutes <= 0 {
        account.disabled_until = None;
        account.disabled_reason = None;
    } else {
        let clamped = minutes.clamp(1, 60 * 24 * 30);
        account.disabled_until = Some(after_minutes(clamped));
        account.disabled_reason = reason
            .map(|r| r.trim().chars().take(200).collect::<String>())
            .filter(|r| !r.is_empty())
            .or_else(|| Some(format!("手动临时禁用 {clamped} 分钟")));
    }
    result(&state.core, state.core.store.save_account(&account))?;
    result(&state.core, state.core.refresh())?;
    Ok(account)
}
#[tauri::command]
async fn delete_account(state: tauri::State<'_, Desktop>, id: String) -> Result<(), String> {
    let _gate = state.core.job_gate.lock().await;
    if state.core.status().busy() {
        return Err("请等待任务完成后删除账号".into());
    }
    result(&state.core, state.core.store.delete_account(&id))?;
    state.core.bridge.remove(&id);
    state.core.music_bridge.remove(&id);
    state.core.browsers.close_all().await;
    result(&state.core, state.core.refresh())
}
#[tauri::command]
async fn sync_account(
    app: tauri::AppHandle,
    state: tauri::State<'_, Desktop>,
    id: String,
) -> Result<Account, String> {
    if state.core.bridge.online(&id) {
        return result(&state.core, state.core.sync_account(&id).await);
    }
    let account = result(&state.core, state.core.store.account(&id))?;
    let ext_dir = state.core.store.root.join("chrome-extension");
    let _ = flow2api_core::export_chrome_extension(&ext_dir);
    let matched = flow2api_core::browser::find_chrome_profile_for_email(&ext_dir, &account.email);
    let _ = open_url_in_chrome_profile(
        app,
        state.clone(),
        matched.as_ref().map(|p| p.profile.clone()),
        Some(account.email.clone()),
        Some("flow".into()),
    );
    for _ in 0..30 {
        tokio::time::sleep(std::time::Duration::from_millis(400)).await;
        if state.core.bridge.ready(&id) {
            return result(&state.core, state.core.store.account(&id));
        }
        if state.core.bridge.online(&id) {
            return result(&state.core, state.core.sync_account(&id).await);
        }
    }
    let msg = match matched {
        Some(prof) if !prof.extension_installed => format!(
            "已在 Chrome（{} · {}）打开 Flow，但该配置尚未安装 Flow2API 插件：请在「本机 Chrome 配置」点击「一键安装插件」将高亮文件夹拖入扩展页即可自动同步",
            prof.name, prof.profile
        ),
        Some(prof) => format!(
            "已在 Chrome（{} · {}）打开 Flow 页面；请确认页面已登录 {} 并保持标签页打开，扩展连接后将自动同步余额",
            prof.name, prof.profile, account.email
        ),
        None => format!(
            "已在浏览器打开 Flow 页面；请确认已安装 Flow2API Chrome 插件并登录 {}，连接后将自动同步余额",
            account.email
        ),
    };
    Err(msg)
}
#[tauri::command]
async fn sync_music_account(
    state: tauri::State<'_, Desktop>,
    id: String,
) -> Result<Account, String> {
    result(&state.core, state.core.sync_music_account(&id).await)
}
#[tauri::command]
fn tasks(state: tauri::State<Desktop>) -> Result<Vec<Task>, String> {
    result(&state.core, state.core.store.task_summaries())
}
#[tauri::command]
fn task_detail(state: tauri::State<Desktop>, id: String) -> Result<Task, String> {
    result(&state.core, state.core.store.task(&id))
}
#[tauri::command]
async fn cancel_task(state: tauri::State<'_, Desktop>, id: String) -> Result<Task, String> {
    result(&state.core, state.core.cancel_task(&id).await)
}
#[tauri::command]
async fn submit(
    state: tauri::State<'_, Desktop>,
    request: GenerationRequest,
) -> Result<Task, String> {
    result(&state.core, state.core.submit(request).await)
}
#[tauri::command]
fn library(state: tauri::State<Desktop>) -> Result<Vec<Media>, String> {
    result(&state.core, state.core.store.media())
}
#[tauri::command]
async fn media_reference(
    state: tauri::State<'_, Desktop>,
    filename: String,
) -> Result<String, String> {
    result(&state.core, state.core.media_reference(&filename).await)
}
#[tauri::command]
async fn choose_legacy_directory(app: tauri::AppHandle) -> Result<Option<String>, String> {
    tokio::task::spawn_blocking(move || {
        app.dialog()
            .file()
            .set_title("选择资料目录或旧版项目目录")
            .blocking_pick_folder()
            .map(|path| {
                path.into_path()
                    .map(|path| path.to_string_lossy().into_owned())
                    .map_err(|e| e.to_string())
            })
            .transpose()
    })
    .await
    .map_err(|e| e.to_string())?
}
#[tauri::command]
async fn preview_legacy(path: String) -> Result<flow2api_core::migration::Preview, String> {
    tokio::task::spawn_blocking(move || {
        flow2api_core::migration::preview(std::path::Path::new(&path))
    })
    .await
    .map_err(|e| e.to_string())?
    .map_err(|e| e.to_string())
}
#[tauri::command]
fn migration_reports(
    state: tauri::State<Desktop>,
) -> Result<Vec<flow2api_core::migration::Report>, String> {
    result(
        &state.core,
        flow2api_core::migration::reports(&state.core.store),
    )
}
#[tauri::command]
fn legacy_history(state: tauri::State<Desktop>, table: String) -> Result<Vec<Value>, String> {
    result(
        &state.core,
        flow2api_core::migration::history(&state.core.store, &table),
    )
}
#[tauri::command]
async fn import_legacy(
    state: tauri::State<'_, Desktop>,
    path: String,
    import_settings: bool,
) -> Result<flow2api_core::migration::Report, String> {
    let core = state.core.clone();
    if core.status().busy() {
        return Err("仍有执行或排队任务，请等待完成后再导入旧版数据".into());
    }
    if matches!(core.status().service.as_str(), "starting" | "stopping") {
        return Err("API 正在切换状态，请稍后再试".into());
    }
    let was_running = core.status().service == "running";
    if was_running {
        result(&core, core.stop_service().await)?;
    }
    let old_max = result(&core, core.store.settings())?.max_concurrent;
    let gate = core.job_gate.lock().await;
    core.browsers.close_all().await;
    let worker = core.clone();
    let outcome = tokio::task::spawn_blocking(move || {
        flow2api_core::migration::import(
            &worker.store,
            std::path::Path::new(&path),
            import_settings,
        )
    })
    .await
    .map_err(|e| e.to_string())
    .and_then(|v| v.map_err(|e| e.to_string()));
    if let Ok(settings) = core.store.settings() {
        core.sync_workers(old_max, settings.max_concurrent);
    }
    let _ = core.refresh();
    drop(gate);
    if was_running {
        let _ = core.start_service().await;
    }
    outcome
}
#[tauri::command]
async fn rollback_legacy(state: tauri::State<'_, Desktop>, id: String) -> Result<(), String> {
    let core = state.core.clone();
    if core.status().busy() {
        return Err("仍有执行或排队任务，请等待完成后再撤销导入".into());
    }
    if matches!(core.status().service.as_str(), "starting" | "stopping") {
        return Err("API 正在切换状态，请稍后再试".into());
    }
    let was_running = core.status().service == "running";
    if was_running {
        result(&core, core.stop_service().await)?;
    }
    let old_max = result(&core, core.store.settings())?.max_concurrent;
    let gate = core.job_gate.lock().await;
    core.browsers.close_all().await;
    let worker = core.clone();
    let outcome =
        tokio::task::spawn_blocking(move || flow2api_core::migration::rollback(&worker.store, &id))
            .await
            .map_err(|e| e.to_string())
            .and_then(|v| v.map_err(|e| e.to_string()));
    if let Ok(settings) = core.store.settings() {
        core.sync_workers(old_max, settings.max_concurrent);
    }
    let _ = core.refresh();
    drop(gate);
    if was_running {
        let _ = core.start_service().await;
    }
    outcome
}
#[tauri::command]
fn open_chrome_extension_dir(
    app: tauri::AppHandle,
    state: tauri::State<Desktop>,
) -> Result<String, String> {
    install_chrome_extension_for_profile(app, state, None).map(|r| r.dir)
}
#[tauri::command]
fn open_userscript_url(app: tauri::AppHandle, state: tauri::State<Desktop>) -> Result<(), String> {
    let settings = state.core.store.settings().unwrap_or_default();
    let port = settings.port;
    let url = format!("http://127.0.0.1:{port}/extension/flow2api-flow-login-sync.user.js");
    let ext_dir = state.core.store.root.join("chrome-extension");
    let preferred_profile = state.core.store.accounts().ok().and_then(|mut list| {
        list.retain(|a| a.enabled);
        list.sort_by(|a, b| b.priority.cmp(&a.priority));
        list.into_iter().find_map(|a| {
            flow2api_core::browser::find_chrome_profile_for_email(&ext_dir, &a.email)
                .map(|p| p.profile)
        })
    });
    let opened = flow2api_core::browser::open_in_chrome_profile(
        &settings.browser_executable,
        preferred_profile.as_deref(),
        &url,
    )
    .map_err(|e| e.to_string())?;
    if !opened {
        app.opener()
            .open_url(&url, None::<&str>)
            .map_err(|e| e.to_string())?;
    }
    Ok(())
}
#[tauri::command]
fn open_data(app: tauri::AppHandle, state: tauri::State<Desktop>) -> Result<(), String> {
    app.opener()
        .open_path(state.core.store.root.to_string_lossy(), None::<&str>)
        .map_err(|e| e.to_string())
}
#[tauri::command]
fn open_media(
    app: tauri::AppHandle,
    state: tauri::State<Desktop>,
    filename: String,
) -> Result<(), String> {
    let media = result(&state.core, state.core.store.media())?;
    if !media.iter().any(|v| v.filename == filename) {
        return Err("作品不存在".into());
    }
    app.opener()
        .open_path(
            state
                .core
                .store
                .root
                .join("generated")
                .join(filename)
                .to_string_lossy(),
            None::<&str>,
        )
        .map_err(|e| e.to_string())
}
#[tauri::command]
fn quit(app: tauri::AppHandle) {
    request_quit(&app);
}
fn apply_autostart(enabled: bool, app: &tauri::AppHandle) -> Result<bool, String> {
    let manager = app.autolaunch();
    let result = (|| {
        if enabled {
            manager.enable()
        } else {
            manager.disable()
        }
        .map_err(|error| format!("无法修改开机自启：{error}"))?;
        let actual = manager
            .is_enabled()
            .map_err(|error| format!("无法读取开机自启：{error}"))?;
        if actual != enabled {
            return Err("系统返回的开机自启状态与请求不一致，请重试".into());
        }
        Ok(actual)
    })();
    if let Ok(slot) = app.state::<Desktop>().autostart_item.lock() {
        if let Some(item) = slot.as_ref() {
            let shown = manager.is_enabled().unwrap_or(false);
            let _ = item.set_checked(shown);
            let _ = item.set_enabled(manager.is_enabled().is_ok());
        }
    }
    result
}
#[tauri::command]
fn autostart_enabled(app: tauri::AppHandle) -> Result<bool, String> {
    app.autolaunch()
        .is_enabled()
        .map_err(|error| format!("无法读取开机自启：{error}"))
}
#[tauri::command]
fn set_autostart(enabled: bool, app: tauri::AppHandle) -> Result<bool, String> {
    apply_autostart(enabled, &app)
}
#[tauri::command]
fn webdav_config(
    state: tauri::State<Desktop>,
) -> Result<flow2api_core::webdav::PublicConfig, String> {
    flow2api_core::webdav::public_config(&state.core.store.root)
}
#[tauri::command]
fn save_webdav(
    state: tauri::State<Desktop>,
    url: String,
    username: String,
    secret: String,
) -> Result<flow2api_core::webdav::PublicConfig, String> {
    flow2api_core::webdav::save_connection(&state.core.store.root, url, username, secret)
}
#[tauri::command]
fn import_verge_webdav(
    state: tauri::State<Desktop>,
) -> Result<flow2api_core::webdav::PublicConfig, String> {
    flow2api_core::webdav::import_verge(&state.core.store.root)
}
#[tauri::command]
async fn webdav_action(state: tauri::State<'_, Desktop>, action: String) -> Result<String, String> {
    let _guard = state.webdav.lock().await;
    if action == "pull" {
        let core = state.core.clone();
        if core.status().busy() {
            return Err("仍有执行或排队任务，请等待完成后再下载云端配置".into());
        }
        if matches!(core.status().service.as_str(), "starting" | "stopping") {
            return Err("API 正在切换状态，请稍后再试".into());
        }
        let was_running = core.status().service == "running";
        if was_running {
            result(&core, core.stop_service().await)?;
        }
        let old_max = result(&core, core.store.settings())?.max_concurrent;
        let gate = core.job_gate.lock().await;
        let outcome = flow2api_core::webdav::perform(&core.store, &action).await;
        if let Ok(settings) = core.store.settings() {
            core.sync_workers(old_max, settings.max_concurrent);
        }
        let _ = core.refresh();
        drop(gate);
        if was_running {
            let _ = core.start_service().await;
        }
        return outcome;
    }
    flow2api_core::webdav::perform(&state.core.store, &action).await
}
fn present(app: &tauri::AppHandle) {
    #[cfg(target_os = "macos")]
    let _ = app.set_activation_policy(tauri::ActivationPolicy::Regular);
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.show();
        let _ = window.unminimize();
        let _ = window.set_focus();
    }
}

/// Hide the workspace and keep the process in the menu bar (macOS) or notification area.
fn park(app: &tauri::AppHandle) {
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.hide();
    }
    #[cfg(target_os = "macos")]
    let _ = app.set_activation_policy(tauri::ActivationPolicy::Accessory);
}
fn request_quit(app: &tauri::AppHandle) {
    if app
        .state::<Desktop>()
        .quit_in_progress
        .swap(true, Ordering::SeqCst)
    {
        return;
    }
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        let state = app.state::<Desktop>();
        match state.core.shutdown().await {
            Ok(()) => {
                state.quitting.store(true, Ordering::SeqCst);
                app.exit(0);
            }
            Err(error) => {
                state.quit_in_progress.store(false, Ordering::SeqCst);
                present(&app);
                let _ = app.emit("notice", format!("无法退出：{error}"));
            }
        }
    });
}
fn tray_icon(status: &Status) -> tauri::image::Image<'static> {
    #[cfg(target_os = "macos")]
    {
        let _ = status;
        return menu_template_icon();
    }
    #[cfg(not(target_os = "macos"))]
    {
        status_dot(status)
    }
}

/// Black “F” with alpha. macOS treats it as a template and tints it for the menu bar.
#[cfg(target_os = "macos")]
fn menu_template_icon() -> tauri::image::Image<'static> {
    const N: u32 = 36;
    let mut rgba = vec![0; (N * N * 4) as usize];
    let mut fill = |x0: u32, y0: u32, x1: u32, y1: u32| {
        for y in y0..y1 {
            for x in x0..x1 {
                let i = ((y * N + x) * 4) as usize;
                rgba[i + 3] = 255;
            }
        }
    };
    fill(8, 6, 14, 30);
    fill(8, 6, 28, 12);
    fill(8, 16, 24, 22);
    tauri::image::Image::new_owned(rgba, N, N)
}

#[cfg(not(target_os = "macos"))]
fn status_dot(status: &Status) -> tauri::image::Image<'static> {
    let color = if status.service == "failed" {
        [240, 112, 104]
    } else if status.busy() {
        [243, 187, 85]
    } else if status.service == "running" {
        [103, 218, 183]
    } else {
        [140, 151, 158]
    };
    let mut rgba = vec![0; 32 * 32 * 4];
    for y in 0..32 {
        for x in 0..32 {
            let dx = x as i32 - 16;
            let dy = y as i32 - 16;
            if dx * dx + dy * dy < 180 {
                let i = (y * 32 + x) * 4;
                rgba[i..i + 3].copy_from_slice(&color);
                rgba[i + 3] = 255;
            }
        }
    }
    tauri::image::Image::new_owned(rgba, 32, 32)
}

pub fn run() {
    let autostart = tauri_plugin_autostart::Builder::new();
    #[cfg(target_os = "macos")]
    let autostart = autostart.macos_launcher(tauri_plugin_autostart::MacosLauncher::LaunchAgent);
    tauri::Builder::default()
        .manage(updates::State::default())
        .plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| {
            present(app);
        }))
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_updater::Builder::new().build())
        .plugin(tauri_plugin_process::init())
        .plugin(autostart.arg("--autostart").build())
        .setup(|app| {
            let data_dir = app.path().app_data_dir()?;
            let background_restart = updates::consume_background_restart(&data_dir);
            let core = App::open(&data_dir)?;
            let status = core.status();
            let auto_update_now = core.store.settings().map(|s| s.auto_update).unwrap_or(true);
            app.manage(Desktop {
                core: core.clone(),
                quitting: AtomicBool::new(false),
                quit_in_progress: AtomicBool::new(false),
                webdav: tokio::sync::Mutex::new(()),
                autostart_item: Mutex::new(None),
                auto_update_item: Mutex::new(None),
                update_menu_item: Mutex::new(None),
            });
            let autostart_now = app.autolaunch().is_enabled();
            let summary =
                MenuItem::with_id(app, "summary", status.tray_summary(), false, None::<&str>)?;
            let details = MenuItem::with_id(app, "details", "等待任务", false, None::<&str>)?;
            let browser = MenuItem::with_id(app, "browser", "浏览器未连接", false, None::<&str>)?;
            let error = MenuItem::with_id(app, "error", "最近错误：无", false, None::<&str>)?;
            let toggle = MenuItem::with_id(app, "toggle", "启动 API 服务", true, None::<&str>)?;
            let check_update_item =
                MenuItem::with_id(app, "check_update", "检查更新…", true, None::<&str>)?;
            let auto_update_item = CheckMenuItem::with_id(
                app,
                "auto_update",
                "自动后台更新",
                true,
                auto_update_now,
                None::<&str>,
            )?;
            let autostart_item = CheckMenuItem::with_id(
                app,
                "autostart",
                "登录后自动启动",
                autostart_now.is_ok(),
                autostart_now.unwrap_or(false),
                None::<&str>,
            )?;
            {
                let desktop = app.state::<Desktop>();
                *desktop.autostart_item.lock().unwrap() = Some(autostart_item.clone());
                *desktop.auto_update_item.lock().unwrap() = Some(auto_update_item.clone());
                *desktop.update_menu_item.lock().unwrap() = Some(check_update_item.clone());
            }
            updates::start(app.handle().clone());
            let menu = Menu::with_items(
                app,
                &[
                    &summary,
                    &details,
                    &browser,
                    &error,
                    &PredefinedMenuItem::separator(app)?,
                    &MenuItem::with_id(app, "show", "打开 Flow2API", true, None::<&str>)?,
                    &toggle,
                    &MenuItem::with_id(app, "data", "打开数据目录", true, None::<&str>)?,
                    &check_update_item,
                    &auto_update_item,
                    &autostart_item,
                    &PredefinedMenuItem::separator(app)?,
                    &MenuItem::with_id(app, "quit", "退出 Flow2API", true, None::<&str>)?,
                ],
            )?;
            #[cfg_attr(not(target_os = "macos"), allow(unused_mut))]
            let mut tray_builder = TrayIconBuilder::with_id("flow2api-status")
                .icon(tray_icon(&status))
                .tooltip(status.tray_summary())
                .menu(&menu)
                .show_menu_on_left_click(cfg!(target_os = "macos"));
            #[cfg(target_os = "macos")]
            {
                tray_builder = tray_builder
                    .icon_as_template(true)
                    .title(status.menu_bar_label());
            }
            let tray = tray_builder
                .on_menu_event(|app, event| match event.id().as_ref() {
                    "show" => present(app),
                    "check_update" => {
                        present(app);
                        let _ = app.emit("check_update", ());
                    }
                    "auto_update" => {
                        let desktop = app.state::<Desktop>();
                        let current = desktop
                            .core
                            .store
                            .settings()
                            .map(|s| s.auto_update)
                            .unwrap_or(true);
                        match desktop.core.set_auto_update(!current) {
                            Ok(actual) => {
                                let status = updates::update_status(
                                    app.clone(),
                                    app.state::<updates::State>(),
                                );
                                sync_update_tray(app, &status);
                                let _ = app.emit("update-status", status);
                                if actual {
                                    updates::wake(app);
                                }
                            }
                            Err(error) => {
                                let _ = app.emit("notice", error.to_string());
                                present(app);
                            }
                        }
                    }
                    "quit" => request_quit(app),
                    "autostart" => {
                        let app = app.clone();
                        let current = app.autolaunch().is_enabled().unwrap_or(false);
                        if let Err(error) = apply_autostart(!current, &app) {
                            let _ = app.emit("notice", error);
                            present(&app);
                        }
                    }
                    "data" => {
                        let root = app.state::<Desktop>().core.store.root.clone();
                        let _ = app.opener().open_path(root.to_string_lossy(), None::<&str>);
                    }
                    "toggle" => {
                        let app = app.clone();
                        tauri::async_runtime::spawn(async move {
                            let state = app.state::<Desktop>();
                            let value = match state.core.status().service.as_str() {
                                "running" => state.core.stop_service().await,
                                "stopped" | "failed" => state.core.start_service().await,
                                // An old menu click can arrive after a transition began.
                                _ => return,
                            };
                            if let Err(error) = value {
                                let _ = app.emit("notice", error.to_string());
                                present(&app);
                            }
                        });
                    }
                    _ => {}
                })
                .on_tray_icon_event(|tray, event| {
                    if cfg!(target_os = "windows")
                        && matches!(
                            event,
                            TrayIconEvent::Click {
                                button: MouseButton::Left,
                                button_state: MouseButtonState::Up,
                                ..
                            }
                        )
                    {
                        present(tray.app_handle());
                    }
                })
                .build(app)?;
            let app_handle = app.handle().clone();
            let mut updates = core.subscribe();
            tauri::async_runtime::spawn(async move {
                while updates.changed().await.is_ok() {
                    let status = updates.borrow_and_update().clone();
                    let _ = summary.set_text(status.tray_summary());
                    let _ = details.set_text(format!(
                        "已完成 {} · 失败／中断 {} · 可用账号 {}/{}",
                        status.succeeded, status.failed, status.ready_accounts, status.accounts
                    ));
                    let _ = browser.set_text(format!("浏览器：{}", status.browser));
                    let _ = error.set_text(format!(
                        "最近错误：{}",
                        status
                            .last_error
                            .as_deref()
                            .unwrap_or("无")
                            .chars()
                            .take(90)
                            .collect::<String>()
                    ));
                    let _ = toggle.set_text(match status.service.as_str() {
                        "running" => "停止 API 服务",
                        "starting" => "API 正在启动…",
                        "stopping" => "API 正在停止…",
                        _ => "启动 API 服务",
                    });
                    let _ = toggle.set_enabled(
                        !status.busy()
                            && matches!(status.service.as_str(), "running" | "stopped" | "failed"),
                    );
                    let _ = tray.set_tooltip(Some(status.tray_summary()));
                    #[cfg(not(target_os = "macos"))]
                    let _ = tray.set_icon(Some(status_dot(&status)));
                    #[cfg(target_os = "macos")]
                    let _ = tray.set_title(Some(status.menu_bar_label()));
                    if let Some(window) = app_handle.get_webview_window("main") {
                        let progress = if status.service == "failed" {
                            tauri::window::ProgressBarStatus::Error
                        } else if status.busy() {
                            tauri::window::ProgressBarStatus::Indeterminate
                        } else {
                            tauri::window::ProgressBarStatus::None
                        };
                        let _ = window.set_progress_bar(tauri::window::ProgressBarState {
                            status: Some(progress),
                            progress: None,
                        });
                        #[cfg(target_os = "macos")]
                        {
                            let _ = window.set_badge_count(if status.busy() {
                                Some((status.running + status.queued) as i64)
                            } else {
                                None
                            });
                        }
                    }
                    let _ = app_handle.emit("status", status);
                }
            });
            tauri::async_runtime::spawn(async move {
                let _ = core.start_service().await;
            });
            if background_restart || std::env::args().any(|arg| arg == "--autostart") {
                park(app.handle());
            } else {
                present(app.handle());
            }
            Ok(())
        })
        .on_window_event(|window, event| {
            if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                api.prevent_close();
                park(window.app_handle());
            }
        })
        .invoke_handler(tauri::generate_handler![
            status,
            settings,
            save_settings,
            magpie_status,
            connect_magpie,
            start_service,
            stop_service,
            models,
            accounts,
            save_account,
            temp_disable_account,
            delete_account,
            sync_account,
            sync_music_account,
            browser_connections,
            music_connections,
            list_chrome_profiles,
            open_url_in_chrome_profile,
            install_chrome_extension_for_profile,
            open_flow_page,
            open_flow_music_page,
            tasks,
            task_detail,
            cancel_task,
            submit,
            library,
            media_reference,
            choose_legacy_directory,
            preview_legacy,
            import_legacy,
            rollback_legacy,
            migration_reports,
            legacy_history,
            open_data,
            open_chrome_extension_dir,
            open_userscript_url,
            open_media,
            quit,
            autostart_enabled,
            set_autostart,
            webdav_config,
            save_webdav,
            import_verge_webdav,
            webdav_action,
            updates::update_status,
            updates::set_auto_update,
            updates::check_update,
            updates::install_update
        ])
        .build(tauri::generate_context!())
        .expect("Flow2API desktop initialization failed")
        .run(|app, event| {
            #[cfg(target_os = "macos")]
            if let tauri::RunEvent::Reopen { .. } = event {
                present(app);
            }
            // Cmd+Q and the Dock Quit item. Closing the window is handled above and only parks the app.
            if let tauri::RunEvent::ExitRequested { api, .. } = event {
                if !app.state::<Desktop>().quitting.load(Ordering::SeqCst) {
                    api.prevent_exit();
                    request_quit(app);
                }
            }
        });
}
