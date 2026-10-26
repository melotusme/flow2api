use flow2api_core::{catalog, model::*, App};
use serde_json::Value;
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};
use tauri::{
    menu::{Menu, MenuItem, PredefinedMenuItem},
    tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent},
    Emitter, Manager,
};
use tauri_plugin_dialog::DialogExt;
use tauri_plugin_opener::OpenerExt;

struct Desktop {
    core: Arc<App>,
    quitting: AtomicBool,
    quit_in_progress: AtomicBool,
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
async fn open_login(state: tauri::State<'_, Desktop>, id: String) -> Result<(), String> {
    result(&state.core, state.core.open_login(&id).await)
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
            .set_title("选择旧版 Flow2API 项目或 data 目录")
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
    let _gate = core.job_gate.lock().await;
    if core.status().busy() || !["stopped", "failed"].contains(&core.status().service.as_str()) {
        return Err("请先停止 API，再导入旧版数据".into());
    }
    core.browsers.close_all().await;
    let worker = core.clone();
    let report = tokio::task::spawn_blocking(move || {
        flow2api_core::migration::import(
            &worker.store,
            std::path::Path::new(&path),
            import_settings,
        )
    })
    .await
    .map_err(|e| e.to_string())?
    .map_err(|e| e.to_string())?;
    result(&core, core.refresh())?;
    Ok(report)
}
#[tauri::command]
async fn rollback_legacy(state: tauri::State<'_, Desktop>, id: String) -> Result<(), String> {
    let core = state.core.clone();
    let _gate = core.job_gate.lock().await;
    if core.status().busy() || !["stopped", "failed"].contains(&core.status().service.as_str()) {
        return Err("请先停止 API，再撤销导入".into());
    }
    core.browsers.close_all().await;
    let worker = core.clone();
    tokio::task::spawn_blocking(move || flow2api_core::migration::rollback(&worker.store, &id))
        .await
        .map_err(|e| e.to_string())?
        .map_err(|e| e.to_string())?;
    result(&core, core.refresh())
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
    tauri::Builder::default()
        .plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| {
            show(app);
        }))
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .setup(|app| {
            let core = App::open(&app.path().app_data_dir()?)?;
            let status = core.status();
            app.manage(Desktop {
                core: core.clone(),
                quitting: AtomicBool::new(false),
                quit_in_progress: AtomicBool::new(false),
            });
            let summary =
                MenuItem::with_id(app, "summary", status.tray_summary(), false, None::<&str>)?;
            let details = MenuItem::with_id(app, "details", "等待任务", false, None::<&str>)?;
            let browser = MenuItem::with_id(app, "browser", "Chrome 未连接", false, None::<&str>)?;
            let error = MenuItem::with_id(app, "error", "最近错误：无", false, None::<&str>)?;
            let toggle = MenuItem::with_id(app, "toggle", "启动 API 服务", true, None::<&str>)?;
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
                    "data" => {
                        let root = app.state::<Desktop>().core.store.root.clone();
                        let _ = app.opener().open_path(root.to_string_lossy(), None::<&str>);
                    }
                    "toggle" => {
                        let app = app.clone();
                        tauri::async_runtime::spawn(async move {
                            let state = app.state::<Desktop>();
                            let value = if state.core.status().service == "running" {
                                state.core.stop_service().await
                            } else {
                                state.core.start_service().await
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
                    let _ = toggle.set_text(if status.service == "running" {
                        "停止 API 服务"
                    } else {
                        "启动 API 服务"
                    });
                    let _ = toggle.set_enabled(!status.busy());
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
            open_login,
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
            open_media,
            quit
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
