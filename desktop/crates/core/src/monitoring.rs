//! Aggregate native runtime metrics. No account identifiers or credential data.
use crate::model::Status;
use std::fmt::Write;

pub fn render(status: &Status) -> String {
    let mut text = String::new();
    for (name, help, value) in [
        (
            "flow2api_up",
            "Whether the local API listener is running.",
            usize::from(status.service == "running"),
        ),
        (
            "flow2api_tokens_total",
            "Number of configured native accounts.",
            status.total_accounts,
        ),
        (
            "flow2api_tokens_active",
            "Enabled accounts with a previously verified Chrome login.",
            status.ready_accounts,
        ),
        (
            "flow2api_tokens_inactive",
            "Accounts that are disabled or require login synchronization.",
            status.total_accounts.saturating_sub(status.ready_accounts),
        ),
        (
            "flow2api_native_accounts_enabled",
            "Number of enabled accounts, including those awaiting login.",
            status.accounts,
        ),
        (
            "flow2api_native_accounts_awaiting_login",
            "Enabled accounts awaiting login synchronization.",
            status.accounts.saturating_sub(status.ready_accounts),
        ),
    ] {
        writeln!(
            text,
            "# HELP {name} {help}\n# TYPE {name} gauge\n{name} {value}"
        )
        .unwrap();
    }
    text.push_str("# HELP flow2api_native_tasks Number of stored native tasks by lifecycle state.\n# TYPE flow2api_native_tasks gauge\n");
    for (state, value) in [
        ("queued", status.queued),
        ("running", status.running),
        ("succeeded", status.succeeded),
        ("failed_or_interrupted", status.failed),
    ] {
        writeln!(text, "flow2api_native_tasks{{state=\"{state}\"}} {value}").unwrap();
    }
    text
}

#[cfg(test)]
mod tests {
    use crate::{model::Account, App};
    use axum::{
        body::{to_bytes, Body},
        http::Request,
    };
    use serde_json::Value;
    use tower::ServiceExt;

    #[tokio::test]
    async fn public_health_and_metrics_distinguish_pending_login_and_invalidation(
    ) -> anyhow::Result<()> {
        let root = tempfile::tempdir()?;
        let app = App::open(root.path())?;
        let mut account = Account {
            id: uuid::Uuid::new_v4().to_string(),
            email: "private@example.test".into(),
            enabled: true,
            project_id: String::new(),
            last_synced: None,
            credits: None,
        };
        app.store.save_account(&account)?;
        app.refresh()?;
        for (enabled, synced) in [(true, false), (true, true), (false, true), (true, false)] {
            account.enabled = enabled;
            account.last_synced = synced.then(crate::model::now);
            app.store.save_account(&account)?;
            app.refresh()?;
            let router = crate::api::router(app.clone());
            let response = router
                .clone()
                .oneshot(Request::builder().uri("/health").body(Body::empty())?)
                .await?;
            assert_eq!(response.status(), 200);
            let health: Value =
                serde_json::from_slice(&to_bytes(response.into_body(), 100_000).await?)?;
            assert_eq!(health["backend_running"], true);
            assert_eq!(health["total_tokens"], 1);
            assert_eq!(health["enabled_accounts"], usize::from(enabled));
            assert_eq!(health["active_tokens"], usize::from(enabled && synced));
            assert_eq!(health["has_active_tokens"], enabled && synced);
            let response = router
                .oneshot(Request::builder().uri("/metrics").body(Body::empty())?)
                .await?;
            assert_eq!(response.status(), 200);
            assert_eq!(
                response.headers()["content-type"],
                "text/plain; version=0.0.4; charset=utf-8"
            );
            let text = String::from_utf8(to_bytes(response.into_body(), 100_000).await?.to_vec())?;
            assert!(text.contains(&format!(
                "flow2api_tokens_active {}\n",
                usize::from(enabled && synced)
            )));
            assert!(text.contains("flow2api_native_tasks{state=\"queued\"} 0\n"));
            assert!(!text.contains(&account.email));
            assert!(!text.contains(&app.store.settings()?.api_key));
        }
        Ok(())
    }
}
