//! Audit a legacy directory using an ephemeral desktop store. Never opens a browser.
use anyhow::Context;
use flow2api_core::{migration, App};
fn main() -> anyhow::Result<()> {
    let path = std::env::args_os().nth(1).context("Usage: cargo run -p flow2api-core --example migration_preview_audit -- <legacy-project-or-data-directory>")?;
    let source = std::path::Path::new(&path);
    let preview = migration::preview(source)?;
    let isolated = tempfile::tempdir()?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(isolated.path(), std::fs::Permissions::from_mode(0o700))?;
    }
    let app = App::open(isolated.path())?;
    let report = migration::import(&app.store, source, false)?;
    anyhow::ensure!(
        report.accounts_added == preview.accounts,
        "account count mismatch"
    );
    anyhow::ensure!(report.media_added == preview.media, "media count mismatch");
    anyhow::ensure!(
        app.store.accounts()?.len() == preview.accounts,
        "stored account count mismatch"
    );
    anyhow::ensure!(
        app.store.media()?.len() == preview.media,
        "stored media count mismatch"
    );
    anyhow::ensure!(report.tasks_added == preview.tasks, "task count mismatch");
    anyhow::ensure!(
        app.store.tasks()?.len() == preview.tasks,
        "stored task count mismatch"
    );
    let audit_db = rusqlite::Connection::open_with_flags(
        isolated.path().join("desktop.db"),
        rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY,
    )?;
    let archived_count = |table: &str| -> rusqlite::Result<usize> {
        audit_db.query_row(
            "SELECT COUNT(*) FROM legacy_records WHERE json_extract(value,'$.table')=?1",
            [table],
            |row| row.get(0),
        )
    };
    anyhow::ensure!(
        archived_count("projects")? == preview.projects,
        "project count mismatch"
    );
    anyhow::ensure!(
        archived_count("request_logs")? == preview.logs,
        "log count mismatch"
    );
    drop(audit_db);
    println!(
        "isolated_import_pass accounts={} projects={} logs={} permanent_media={}",
        report.accounts_added, preview.projects, preview.logs, report.media_added
    );
    migration::rollback(&app.store, &report.id)?;
    anyhow::ensure!(
        app.store.accounts()?.is_empty(),
        "rollback account mismatch"
    );
    anyhow::ensure!(app.store.media()?.is_empty(), "rollback media mismatch");
    anyhow::ensure!(
        migration::history(&app.store, "projects")?.is_empty(),
        "rollback project mismatch"
    );
    anyhow::ensure!(app.store.tasks()?.is_empty(), "rollback task mismatch");
    anyhow::ensure!(
        migration::history(&app.store, "request_logs")?.is_empty(),
        "rollback log mismatch"
    );
    println!("isolated_rollback_pass; desktop data untouched; no generation or login");
    Ok(())
}
