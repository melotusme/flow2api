pub mod api;
pub mod browser;
pub mod catalog;
pub mod flow;
pub mod generation;
pub mod migration;
pub mod model;
mod monitoring;
mod request;
pub mod resolver;
pub mod store;
mod video;

use anyhow::{bail, Result};
use model::*;
use std::{
    path::Path,
    sync::{Arc, Mutex},
};
use tokio::sync::{oneshot, watch, Mutex as AsyncMutex};

struct Server {
    shutdown: oneshot::Sender<()>,
    done: tokio::task::JoinHandle<()>,
}

pub struct App {
    pub store: store::Store,
    status: Mutex<Status>,
    updates: watch::Sender<Status>,
    server: AsyncMutex<Option<Server>>,
    pub browsers: browser::BrowserPool,
    pub job_gate: AsyncMutex<()>,
    pub workers: Arc<tokio::sync::Semaphore>,
}

impl App {
    pub fn open(root: &Path) -> Result<Arc<Self>> {
        let store = store::Store::open(root)?;
        let status = Status {
            service: "stopped".into(),
            browser: "未连接".into(),
            data_dir: root.display().to_string(),
            ..Default::default()
        };
        let (updates, _) = watch::channel(status.clone());
        let workers = Arc::new(tokio::sync::Semaphore::new(
            store.settings()?.max_concurrent,
        ));
        let app = Arc::new(Self {
            browsers: browser::BrowserPool::new(root.join("profiles")),
            store,
            status: Mutex::new(status),
            updates,
            server: AsyncMutex::new(None),
            job_gate: AsyncMutex::new(()),
            workers,
        });
        app.refresh()?;
        Ok(app)
    }
    pub fn status(&self) -> Status {
        self.status.lock().unwrap().clone()
    }
    pub fn subscribe(&self) -> watch::Receiver<Status> {
        self.updates.subscribe()
    }
    pub fn change_status(&self, update: impl FnOnce(&mut Status)) {
        let mut status = self.status.lock().unwrap();
        update(&mut status);
        self.updates.send_replace(status.clone());
    }
    pub fn refresh(&self) -> Result<()> {
        let tasks = self.store.tasks()?;
        let accounts = self.store.accounts()?;
        self.change_status(|s| {
            s.total_accounts = accounts.len();
            s.accounts = accounts.iter().filter(|a| a.enabled).count();
            s.ready_accounts = accounts
                .iter()
                .filter(|a| a.enabled && a.last_synced.is_some())
                .count();
            s.queued = tasks.iter().filter(|t| t.state == "queued").count();
            s.running = tasks.iter().filter(|t| t.state == "running").count();
            s.succeeded = tasks.iter().filter(|t| t.state == "succeeded").count();
            s.failed = tasks
                .iter()
                .filter(|t| t.state == "failed" || t.state == "interrupted")
                .count();
        });
        Ok(())
    }
    pub async fn start_service(self: &Arc<Self>) -> Result<Status> {
        let _admission = self.job_gate.lock().await;
        let mut slot = self.server.lock().await;
        if slot.as_ref().is_some_and(|v| !v.done.is_finished()) {
            return Ok(self.status());
        }
        self.change_status(|s| {
            s.service = "starting".into();
            s.last_error = None;
        });
        let port = self.store.settings()?.port;
        let listener =
            match tokio::net::TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, port)).await {
                Ok(listener) => listener,
                Err(_) => {
                    let message = format!("端口 {port} 无法监听，请停止旧实例或在设置中修改端口");
                    self.change_status(|s| {
                        s.service = "failed".into();
                        s.address = None;
                        s.last_error = Some(message.clone());
                    });
                    bail!(message);
                }
            };
        let (shutdown, receiver) = oneshot::channel();
        let app = self.clone();
        self.change_status(|s| {
            s.service = "running".into();
            s.address = Some(format!("http://127.0.0.1:{port}"));
        });
        let done = tokio::spawn(async move {
            let result = axum::serve(listener, api::router(app.clone()))
                .with_graceful_shutdown(async {
                    let _ = receiver.await;
                })
                .await;
            app.change_status(|s| {
                s.address = None;
                s.service = if result.is_ok() { "stopped" } else { "failed" }.into();
                if let Err(error) = result {
                    s.last_error = Some(format!("HTTP 服务退出: {error}"));
                }
            });
        });
        *slot = Some(Server { shutdown, done });
        Ok(self.status())
    }
    pub async fn stop_service(&self) -> Result<Status> {
        let _admission = self.job_gate.lock().await;
        // Serialize lifecycle with starts and keep the actual listener's join handle.
        let mut slot = self.server.lock().await;
        if self.status().busy() {
            bail!("仍有执行或排队任务，请等待完成后停止服务");
        }
        if let Some(server) = slot.take() {
            self.change_status(|s| s.service = "stopping".into());
            let _ = server.shutdown.send(());
            server.done.await?;
        }
        Ok(self.status())
    }
    pub async fn shutdown(&self) -> Result<()> {
        self.stop_service().await?;
        self.browsers.close_all().await;
        self.change_status(|s| s.browser = "已关闭".into());
        Ok(())
    }
    pub async fn save_settings(&self, settings: Settings) -> Result<()> {
        let _admission = self.job_gate.lock().await;
        if self.status().busy() || self.status().service == "running" {
            bail!("请先停止 API，再修改运行设置");
        }
        let old = self.store.settings()?.max_concurrent;
        self.store.save_settings(&settings)?;
        if settings.max_concurrent > old {
            self.workers.add_permits(settings.max_concurrent - old);
        } else {
            self.workers.forget_permits(old - settings.max_concurrent);
        }
        self.browsers.close_all().await;
        Ok(())
    }
}
