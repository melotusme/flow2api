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
async fn save_settings(state: tauri::State<'_, Desktop>, settings: Settings) -> Result<(), String> {
    result(&state.core, state.core.save_settings(settings).await)
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
async fn save_account(
    state: tauri::State<'_, Desktop>,
    email: String,
    id: Option<String>,
    enabled: bool,
) -> Result<Account, String> {
    let _gate = state.core.job_gate.lock().await;
    if state.core.status().busy() {
        return Err("请等待任务完成后修改账号".into());
    }
    let mut account = match id {
        Some(id) => result(&state.core, state.core.store.account(&id))?,
        None => Account {
            id: uuid::Uuid::new_v4().to_string(),
            email: email.clone(),
            enabled,
            project_id: String::new(),
            last_synced: None,
            credits: None,
            browser_data_dir: String::new(),
            browser_profile: String::new(),
        },
    };
    if account.email != email {
        account.last_synced = None;
        account.credits = None;
    }
    account.email = email.trim().into();
    account.enabled = enabled;
    result(&state.core, state.core.store.save_account(&account))?;
    result(&state.core, state.core.refresh())?;
    Ok(account)
}
#[tauri::command]
async fn configure_account_browser(
    state: tauri::State<'_, Desktop>,
    id: String,
    data_dir: String,
    profile: String,
) -> Result<(), String> {
    let _gate = state.core.job_gate.lock().await;
    if state.core.status().busy() {
        return Err("请等待任务完成后修改浏览器资料".into());
    }
    let mut account = result(&state.core, state.core.store.account(&id))?;
    if !data_dir.is_empty() {
        flow2api_core::browser::validate_profile(&data_dir, &profile).map_err(|e| e.to_string())?;
    }
    state.core.browsers.close_all().await;
    account.browser_data_dir = data_dir;
    account.browser_profile = if account.browser_data_dir.is_empty() {
        String::new()
    } else {
        profile
    };
    account.last_synced = None;
    account.credits = None;
    result(&state.core, state.core.store.save_account(&account))?;
    result(&state.core, state.core.refresh())
}
#[tauri::command]
async fn browser_profiles() -> Result<Vec<flow2api_core::browser::BrowserProfile>, String> {
    tokio::task::spawn_blocking(flow2api_core::browser::discover_profiles)
        .await
        .map_err(|e| e.to_string())
}
#[tauri::command]
async fn choose_browser(app: tauri::AppHandle) -> Result<Option<String>, String> {
    tokio::task::spawn_blocking(move || {
        app.dialog()
            .file()
            .set_title("选择 Chrome 程序（Mac 可选择 Google Chrome.app）")
            .blocking_pick_file()
            .map(|f| f.into_path().map_err(|e| e.to_string()))
            .transpose()
            .map(|f| f.map(|p| p.to_string_lossy().into_owned()))
    })
    .await
    .map_err(|e| e.to_string())?
}
#[tauri::command]
async fn delete_account(state: tauri::State<'_, Desktop>, id: String) -> Result<(), String> {
    let _gate = state.core.job_gate.lock().await;
    if state.core.status().busy() {
        return Err("请等待任务完成后删除账号".into());
    }
    result(&state.core, state.core.store.delete_account(&id))?;
    state.core.browsers.close_all().await;
    result(&state.core, state.core.refresh())
}
#[tauri::command]
async fn open_login(
    app: tauri::AppHandle,
    state: tauri::State<'_, Desktop>,
    id: String,
) -> Result<(), String> {
    result(&state.core, state.core.open_login(&id).await)?;
    watch_login(app, state.core.clone(), id, false);
    Ok(())
}
/// One click: open a fresh Chrome on Google sign-in; the email is detected automatically.
#[tauri::command]
async fn quick_login(
    app: tauri::AppHandle,
    state: tauri::State<'_, Desktop>,
) -> Result<(), String> {
    let id = result(&state.core, state.core.start_quick_login().await)?;
    watch_login(app, state.core.clone(), id, true);
    Ok(())
}
/// Poll the login window for up to 10 minutes. `pending` means the account is not saved
/// yet: first detect the email and create it, then keep syncing until credits are readable.
fn watch_login(app: tauri::AppHandle, core: Arc<App>, id: String, mut pending: bool) {
    tauri::async_runtime::spawn(async move {
        for _ in 0..200 {
            tokio::time::sleep(std::time::Duration::from_secs(3)).await;
            if core.browsers.existing_session(&id).await.is_none() {
                break;
            }
            let synced = if pending {
                match core.try_finish_quick_login(&id).await {
                    Ok(Some(account)) => {
                        pending = false;
                        let _ = app.emit("account-synced", &account);
                        if account.last_synced.is_none() {
                            let _ = app.emit(
                                "notice",
                                format!("已识别 {}，正在等待 Flow 页面就绪后同步…", account.email),
                            );
                            continue;
                        }
                        Some(account)
                    }
                    Ok(None) => None,
                    Err(error) => {
                        let _ = app.emit("notice", error.to_string());
                        core.browsers.discard(&id).await;
                        return;
                    }
                }
            } else {
                core.try_auto_sync(&id).await.ok().flatten()
            };
            if let Some(account) = synced {
                let _ = app.emit("account-synced", &account);
                let _ = app.emit(
                    "notice",
                    format!("{} 登录成功，已自动同步。", account.email),
                );
                return;
            }
        }
        // The window was closed (or timed out) before an email was detected: nothing to keep.
        if pending {
            core.browsers.discard(&id).await;
        }
    });
}
#[tauri::command]
async fn sync_account(state: tauri::State<'_, Desktop>, id: String) -> Result<Account, String> {
    result(&state.core, state.core.sync_account(&id).await)
}
#[tauri::command]
fn tasks(state: tauri::State<Desktop>) -> Result<Vec<Task>, String> {
    result(&state.core, state.core.store.tasks())
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
fn open_chrome_download(app: tauri::AppHandle) -> Result<(), String> {
    app.opener()
        .open_url("https://www.google.com/chrome/", None::<&str>)
        .map_err(|e| e.to_string())
}
fn ensure_extension_files(root: &std::path::Path) -> Result<std::path::PathBuf, String> {
    let dir = root.join("extension");
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let _ = std::fs::write(
        dir.join("manifest.json"),
        include_str!("../../crates/core/assets/extension/manifest.json"),
    );
    let _ = std::fs::write(
        dir.join("popup.html"),
        include_str!("../../crates/core/assets/extension/popup.html"),
    );
    let _ = std::fs::write(
        dir.join("popup.js"),
        include_str!("../../crates/core/assets/extension/popup.js"),
    );
    let _ = std::fs::write(
        dir.join("content.js"),
        include_str!("../../crates/core/assets/extension/content.js"),
    );
    let _ = std::fs::write(
        dir.join("background.js"),
        include_str!("../../crates/core/assets/extension/background.js"),
    );
    let _ = std::fs::write(
        dir.join("options.html"),
        include_str!("../../crates/core/assets/extension/options.html"),
    );
    let _ = std::fs::write(
        dir.join("options.js"),
        include_str!("../../crates/core/assets/extension/options.js"),
    );
    let _ = std::fs::write(
        dir.join("flow2api-flow-login-sync.user.js"),
        include_str!("../../crates/core/assets/extension/flow2api-flow-login-sync.user.js"),
    );
    Ok(dir)
}
#[tauri::command]
async fn install_chrome_extension(
    _app: tauri::AppHandle,
    state: tauri::State<'_, Desktop>,
) -> Result<String, String> {
    let root = state.core.store.root.clone();
    let ext_dir = ensure_extension_files(&root)?;
    let settings = result(&state.core, state.core.store.settings())?;
    let executable = flow2api_core::browser::executable(&settings.browser_executable)
        .map_err(|e| e.to_string())?;

    let mut command = std::process::Command::new(executable);
    command.arg(format!("--load-extension={}", ext_dir.display()));
    command.arg("https://flow.google.com/");
    command
        .spawn()
        .map_err(|e| format!("启动 Chrome 失败: {e}"))?;

    Ok(ext_dir.to_string_lossy().into_owned())
}
#[tauri::command]
fn open_extension_folder(
    app: tauri::AppHandle,
    state: tauri::State<Desktop>,
) -> Result<String, String> {
    let root = state.core.store.root.clone();
    let ext_dir = ensure_extension_files(&root)?;
    let _ = app
        .opener()
        .open_path(ext_dir.to_string_lossy(), None::<&str>);
    Ok(ext_dir.to_string_lossy().into_owned())
}
#[tauri::command]
fn open_userscript_url(app: tauri::AppHandle, state: tauri::State<Desktop>) -> Result<(), String> {
    let port = state.core.store.settings().map(|s| s.port).unwrap_or(8000);
    let url = format!("http://127.0.0.1:{port}/extension/flow2api-flow-login-sync.user.js");
    app.opener()
        .open_url(&url, None::<&str>)
        .map_err(|e| e.to_string())
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
fn show(app: &tauri::AppHandle) {
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.show();
        let _ = window.unminimize();
        let _ = window.set_focus();
    }
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
                show(&app);
                let _ = app.emit("notice", format!("无法退出：{error}"));
            }
        }
    });
}
fn icon(status: &Status) -> tauri::image::Image<'static> {
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
            show(app);
        }))
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_updater::Builder::new().build())
        .plugin(tauri_plugin_process::init())
        .plugin(autostart.arg("--autostart").build())
        .setup(|app| {
            let core = App::open(&app.path().app_data_dir()?)?;
            let status = core.status();
            app.manage(Desktop {
                core: core.clone(),
                quitting: AtomicBool::new(false),
                quit_in_progress: AtomicBool::new(false),
                webdav: tokio::sync::Mutex::new(()),
                autostart_item: Mutex::new(None),
            });
            let autostart_now = app.autolaunch().is_enabled();
            let summary =
                MenuItem::with_id(app, "summary", status.tray_summary(), false, None::<&str>)?;
            let details = MenuItem::with_id(app, "details", "等待任务", false, None::<&str>)?;
            let browser = MenuItem::with_id(app, "browser", "Chrome 未连接", false, None::<&str>)?;
            let error = MenuItem::with_id(app, "error", "最近错误：无", false, None::<&str>)?;
            let toggle = MenuItem::with_id(app, "toggle", "启动 API 服务", true, None::<&str>)?;
            let autostart_item = CheckMenuItem::with_id(
                app,
                "autostart",
                "登录后自动启动",
                autostart_now.is_ok(),
                autostart_now.unwrap_or(false),
                None::<&str>,
            )?;
            *app.state::<Desktop>().autostart_item.lock().unwrap() = Some(autostart_item.clone());
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
                    &autostart_item,
                    &PredefinedMenuItem::separator(app)?,
                    &MenuItem::with_id(app, "quit", "退出 Flow2API", true, None::<&str>)?,
                ],
            )?;
            let tray = TrayIconBuilder::with_id("flow2api-status")
                .icon(icon(&status))
                .tooltip(status.tray_summary())
                .menu(&menu)
                .show_menu_on_left_click(cfg!(target_os = "macos"))
                .on_menu_event(|app, event| match event.id().as_ref() {
                    "show" => show(app),
                    "quit" => request_quit(app),
                    "autostart" => {
                        let app = app.clone();
                        let current = app.autolaunch().is_enabled().unwrap_or(false);
                        if let Err(error) = apply_autostart(!current, &app) {
                            let _ = app.emit("notice", error);
                            show(&app);
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
                                show(&app);
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
                        show(tray.app_handle());
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
                    let _ = tray.set_icon(Some(icon(&status)));
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
                    #[cfg(target_os = "macos")]
                    {
                        let _ = tray.set_title(Some(if status.busy() {
                            format!("F {}", status.running + status.queued)
                        } else {
                            "F".into()
                        }));
                    }
                    let _ = app_handle.emit("status", status);
                }
            });
            tauri::async_runtime::spawn(async move {
                let _ = core.start_service().await;
            });
            if !std::env::args().any(|arg| arg == "--autostart") {
                show(app.handle());
            }
            Ok(())
        })
        .on_window_event(|window, event| {
            if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                api.prevent_close();
                let _ = window.hide();
            }
        })
        .invoke_handler(tauri::generate_handler![
            status,
            settings,
            save_settings,
            start_service,
            stop_service,
            models,
            accounts,
            save_account,
            delete_account,
            configure_account_browser,
            browser_profiles,
            choose_browser,
            open_login,
            quick_login,
            sync_account,
            tasks,
            submit,
            library,
            choose_legacy_directory,
            preview_legacy,
            import_legacy,
            rollback_legacy,
            migration_reports,
            legacy_history,
            open_data,
            open_chrome_download,
            install_chrome_extension,
            open_extension_folder,
            open_userscript_url,
            open_media,
            quit,
            autostart_enabled,
            set_autostart,
            webdav_config,
            save_webdav,
            import_verge_webdav,
            webdav_action,
            updates::check_private_update,
            updates::install_private_update
        ])
        .build(tauri::generate_context!())
        .expect("Flow2API desktop initialization failed")
        .run(|app, event| {
            #[cfg(target_os = "macos")]
            if let tauri::RunEvent::Reopen { .. } = event {
                show(app);
            }
            if let tauri::RunEvent::ExitRequested { api, .. } = event {
                if !app.state::<Desktop>().quitting.load(Ordering::SeqCst) {
                    api.prevent_exit();
                    request_quit(app);
                }
            }
        });
}
