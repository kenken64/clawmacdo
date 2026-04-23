use anyhow::{Context, Result};
use rusqlite::Connection;
use serde::Serialize;
use std::path::PathBuf;

/// Return the path to the SQLite database: ~/.clawmacdo/deployments.db
fn db_path() -> Result<PathBuf> {
    let home = dirs::home_dir().context("Cannot determine home directory")?;
    let dir = home.join(".clawmacdo");
    std::fs::create_dir_all(&dir)?;
    Ok(dir.join("deployments.db"))
}

/// Open (or create) the database and ensure the schema exists.
pub fn init_db() -> Result<Connection> {
    let path = db_path()?;
    let conn = Connection::open(&path)
        .with_context(|| format!("Failed to open SQLite database at {}", path.display()))?;

    conn.execute_batch("PRAGMA journal_mode=WAL;")
        .context("Failed to enable WAL mode")?;

    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS deployments (
            id              TEXT PRIMARY KEY,
            customer_name   TEXT NOT NULL,
            customer_email  TEXT NOT NULL,
            provider        TEXT,
            hostname        TEXT,
            ip_address      TEXT,
            region          TEXT,
            size            TEXT,
            status          TEXT NOT NULL DEFAULT 'running',
            created_at      TEXT NOT NULL
        );",
    )
    .context("Failed to create deployments table")?;

    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS deploy_steps (
            id           INTEGER PRIMARY KEY AUTOINCREMENT,
            deploy_id    TEXT NOT NULL,
            step_number  INTEGER NOT NULL,
            total_steps  INTEGER NOT NULL DEFAULT 16,
            label        TEXT NOT NULL,
            status       TEXT NOT NULL DEFAULT 'running',
            started_at   TEXT NOT NULL,
            completed_at TEXT,
            error_msg    TEXT,
            UNIQUE(deploy_id, step_number)
        );",
    )
    .context("Failed to create deploy_steps table")?;

    Ok(conn)
}

/// Insert a new deployment row when a deploy starts.
#[allow(clippy::too_many_arguments)]
pub fn insert_deployment(
    conn: &Connection,
    id: &str,
    customer_name: &str,
    customer_email: &str,
    provider: &str,
    region: &str,
    size: &str,
    hostname: &str,
) -> Result<()> {
    conn.execute(
        "INSERT INTO deployments (id, customer_name, customer_email, provider, region, size, hostname, status, created_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, 'running', datetime('now'))",
        rusqlite::params![id, customer_name, customer_email, provider, region, size, hostname],
    )
    .context("Failed to insert deployment")?;
    Ok(())
}

/// Update a deployment's status (and optionally ip/hostname) on completion or failure.
pub fn update_deployment_status(
    conn: &Connection,
    id: &str,
    status: &str,
    ip_address: Option<&str>,
    hostname: Option<&str>,
) -> Result<()> {
    conn.execute(
        "UPDATE deployments SET status = ?1, ip_address = COALESCE(?2, ip_address), hostname = COALESCE(?3, hostname) WHERE id = ?4",
        rusqlite::params![status, ip_address, hostname, id],
    )
    .context("Failed to update deployment status")?;
    Ok(())
}

/// List deployments with pagination.
pub fn list_deployments_paginated(
    conn: &Connection,
    page: u32,
    per_page: u32,
) -> Result<(Vec<DeploymentRow>, u32)> {
    let total: u32 = conn.query_row("SELECT COUNT(*) FROM deployments", [], |row| row.get(0))?;
    let offset = (page.saturating_sub(1)) * per_page;
    let mut stmt = conn.prepare(
        "SELECT id, customer_name, customer_email, provider, hostname, ip_address, region, size, status, created_at
         FROM deployments ORDER BY created_at DESC LIMIT ?1 OFFSET ?2",
    )?;
    let rows = stmt
        .query_map(rusqlite::params![per_page, offset], |row| {
            Ok(DeploymentRow {
                id: row.get(0)?,
                customer_name: row.get(1)?,
                customer_email: row.get(2)?,
                provider: row.get(3)?,
                hostname: row.get(4)?,
                ip_address: row.get(5)?,
                region: row.get(6)?,
                size: row.get(7)?,
                status: row.get(8)?,
                created_at: row.get(9)?,
            })
        })?
        .collect::<Result<Vec<_>, _>>()?;
    Ok((rows, total))
}

/// Delete a single deployment by ID.
pub fn delete_deployment(conn: &Connection, id: &str) -> Result<bool> {
    let changed = conn.execute(
        "DELETE FROM deployments WHERE id = ?1",
        rusqlite::params![id],
    )?;
    Ok(changed > 0)
}

#[derive(Serialize)]
pub struct DeploymentRow {
    pub id: String,
    pub customer_name: String,
    pub customer_email: String,
    pub provider: Option<String>,
    pub hostname: Option<String>,
    pub ip_address: Option<String>,
    pub region: Option<String>,
    pub size: Option<String>,
    pub status: String,
    pub created_at: String,
}

#[derive(Debug, Serialize)]
pub struct DeployStepRow {
    pub deploy_id: String,
    pub step_number: i32,
    pub total_steps: i32,
    pub label: String,
    pub status: String,
    pub started_at: String,
    pub completed_at: Option<String>,
    pub error_msg: Option<String>,
}

// ── Deploy step CRUD ────────────────────────────────────────────────────────

pub fn insert_deploy_step(
    conn: &Connection,
    deploy_id: &str,
    step_number: i32,
    total_steps: i32,
    label: &str,
) -> Result<()> {
    conn.execute(
        "INSERT OR REPLACE INTO deploy_steps (deploy_id, step_number, total_steps, label, status, started_at)
         VALUES (?1, ?2, ?3, ?4, 'running', datetime('now'))",
        rusqlite::params![deploy_id, step_number, total_steps, label],
    )
    .context("Failed to insert deploy step")?;
    Ok(())
}

pub fn complete_deploy_step(conn: &Connection, deploy_id: &str, step_number: i32) -> Result<()> {
    conn.execute(
        "UPDATE deploy_steps SET status = 'completed', completed_at = datetime('now') WHERE deploy_id = ?1 AND step_number = ?2",
        rusqlite::params![deploy_id, step_number],
    )
    .context("Failed to complete deploy step")?;
    Ok(())
}

pub fn fail_deploy_step(
    conn: &Connection,
    deploy_id: &str,
    step_number: i32,
    error_msg: &str,
) -> Result<()> {
    conn.execute(
        "UPDATE deploy_steps SET status = 'failed', completed_at = datetime('now'), error_msg = ?3 WHERE deploy_id = ?1 AND step_number = ?2",
        rusqlite::params![deploy_id, step_number, error_msg],
    )
    .context("Failed to fail deploy step")?;
    Ok(())
}

pub fn skip_deploy_step(conn: &Connection, deploy_id: &str, step_number: i32) -> Result<()> {
    conn.execute(
        "UPDATE deploy_steps SET status = 'skipped', completed_at = datetime('now') WHERE deploy_id = ?1 AND step_number = ?2",
        rusqlite::params![deploy_id, step_number],
    )
    .context("Failed to skip deploy step")?;
    Ok(())
}

pub fn get_deploy_steps(conn: &Connection, deploy_id: &str) -> Result<Vec<DeployStepRow>> {
    let mut stmt = conn.prepare(
        "SELECT deploy_id, step_number, total_steps, label, status, started_at, completed_at, error_msg
         FROM deploy_steps WHERE deploy_id = ?1 ORDER BY step_number",
    )?;
    let rows = stmt
        .query_map(rusqlite::params![deploy_id], |row| {
            Ok(DeployStepRow {
                deploy_id: row.get(0)?,
                step_number: row.get(1)?,
                total_steps: row.get(2)?,
                label: row.get(3)?,
                status: row.get(4)?,
                started_at: row.get(5)?,
                completed_at: row.get(6)?,
                error_msg: row.get(7)?,
            })
        })?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(rows)
}

// ── Convenience wrappers for optional Db handles ────────────────────────────
// These accept `Option<Arc<Mutex<Connection>>>` so callers (snapshot, restore,
// deploy commands) can pass `None` in CLI mode and `Some(db)` in web/serve mode
// without duplicating the lock-and-ignore-error boilerplate.

use std::sync::{Arc, Mutex};

pub fn record_step_start(
    db: &Option<Arc<Mutex<Connection>>>,
    op_id: &str,
    step: i32,
    total: i32,
    label: &str,
) {
    if let Some(db) = db {
        if let Ok(conn) = db.lock() {
            let _ = insert_deploy_step(&conn, op_id, step, total, label);
        }
    }
}

pub fn record_step_complete(db: &Option<Arc<Mutex<Connection>>>, op_id: &str, step: i32) {
    if let Some(db) = db {
        if let Ok(conn) = db.lock() {
            let _ = complete_deploy_step(&conn, op_id, step);
        }
    }
}

pub fn record_step_failed(
    db: &Option<Arc<Mutex<Connection>>>,
    op_id: &str,
    step: i32,
    error_msg: &str,
) {
    if let Some(db) = db {
        if let Ok(conn) = db.lock() {
            let _ = fail_deploy_step(&conn, op_id, step, error_msg);
        }
    }
}

pub fn record_step_skipped(db: &Option<Arc<Mutex<Connection>>>, op_id: &str, step: i32) {
    if let Some(db) = db {
        if let Ok(conn) = db.lock() {
            let _ = skip_deploy_step(&conn, op_id, step);
        }
    }
}

// ── Single deployment lookups ───────────────────────────────────────────────

pub fn get_deployment_by_id(conn: &Connection, id: &str) -> Result<Option<DeploymentRow>> {
    let mut stmt = conn.prepare(
        "SELECT id, customer_name, customer_email, provider, hostname, ip_address, region, size, status, created_at
         FROM deployments WHERE id = ?1",
    )?;
    let mut rows = stmt.query_map(rusqlite::params![id], |row| {
        Ok(DeploymentRow {
            id: row.get(0)?,
            customer_name: row.get(1)?,
            customer_email: row.get(2)?,
            provider: row.get(3)?,
            hostname: row.get(4)?,
            ip_address: row.get(5)?,
            region: row.get(6)?,
            size: row.get(7)?,
            status: row.get(8)?,
            created_at: row.get(9)?,
        })
    })?;
    match rows.next() {
        Some(row) => Ok(Some(row?)),
        None => Ok(None),
    }
}

pub fn find_deployment_by_query(conn: &Connection, query: &str) -> Result<Option<DeploymentRow>> {
    // Empty query → return most recent deployment
    if query.is_empty() {
        let mut stmt = conn.prepare(
            "SELECT id, customer_name, customer_email, provider, hostname, ip_address, region, size, status, created_at
             FROM deployments ORDER BY created_at DESC LIMIT 1",
        )?;
        let mut rows = stmt.query_map([], |row| {
            Ok(DeploymentRow {
                id: row.get(0)?,
                customer_name: row.get(1)?,
                customer_email: row.get(2)?,
                provider: row.get(3)?,
                hostname: row.get(4)?,
                ip_address: row.get(5)?,
                region: row.get(6)?,
                size: row.get(7)?,
                status: row.get(8)?,
                created_at: row.get(9)?,
            })
        })?;
        return match rows.next() {
            Some(row) => Ok(Some(row?)),
            None => Ok(None),
        };
    }
    // Try exact ID match first, then hostname, then IP
    if let Some(row) = get_deployment_by_id(conn, query)? {
        return Ok(Some(row));
    }
    let mut stmt = conn.prepare(
        "SELECT id, customer_name, customer_email, provider, hostname, ip_address, region, size, status, created_at
         FROM deployments WHERE hostname = ?1 OR ip_address = ?1 ORDER BY created_at DESC LIMIT 1",
    )?;
    let mut rows = stmt.query_map(rusqlite::params![query], |row| {
        Ok(DeploymentRow {
            id: row.get(0)?,
            customer_name: row.get(1)?,
            customer_email: row.get(2)?,
            provider: row.get(3)?,
            hostname: row.get(4)?,
            ip_address: row.get(5)?,
            region: row.get(6)?,
            size: row.get(7)?,
            status: row.get(8)?,
            created_at: row.get(9)?,
        })
    })?;
    match rows.next() {
        Some(row) => Ok(Some(row?)),
        None => Ok(None),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_conn() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(
            "CREATE TABLE deployments (
                id              TEXT PRIMARY KEY,
                customer_name   TEXT NOT NULL,
                customer_email  TEXT NOT NULL,
                provider        TEXT,
                hostname        TEXT,
                ip_address      TEXT,
                region          TEXT,
                size            TEXT,
                status          TEXT NOT NULL DEFAULT 'running',
                created_at      TEXT NOT NULL
            );
            CREATE TABLE deploy_steps (
                id           INTEGER PRIMARY KEY AUTOINCREMENT,
                deploy_id    TEXT NOT NULL,
                step_number  INTEGER NOT NULL,
                total_steps  INTEGER NOT NULL DEFAULT 16,
                label        TEXT NOT NULL,
                status       TEXT NOT NULL DEFAULT 'running',
                started_at   TEXT NOT NULL,
                completed_at TEXT,
                error_msg    TEXT,
                UNIQUE(deploy_id, step_number)
            );",
        )
        .unwrap();
        conn
    }

    fn insert_sample_deployment(conn: &Connection, id: &str, hostname: &str, ip: &str) {
        insert_deployment(
            conn,
            id,
            "Jane Doe",
            "jane@example.com",
            "digitalocean",
            "sgp1",
            "s-2vcpu-4gb",
            hostname,
        )
        .unwrap();
        update_deployment_status(conn, id, "completed", Some(ip), Some(hostname)).unwrap();
    }

    #[test]
    fn deployment_lookup_matches_id_hostname_and_ip() {
        let conn = test_conn();
        insert_sample_deployment(&conn, "deploy-1", "openclaw-one", "203.0.113.10");

        assert_eq!(
            get_deployment_by_id(&conn, "deploy-1")
                .unwrap()
                .unwrap()
                .hostname,
            Some("openclaw-one".to_string())
        );
        assert_eq!(
            find_deployment_by_query(&conn, "openclaw-one")
                .unwrap()
                .unwrap()
                .id,
            "deploy-1"
        );
        assert_eq!(
            find_deployment_by_query(&conn, "203.0.113.10")
                .unwrap()
                .unwrap()
                .id,
            "deploy-1"
        );
    }

    #[test]
    fn empty_query_returns_most_recent_deployment() {
        let conn = test_conn();
        insert_sample_deployment(&conn, "deploy-older", "older-host", "203.0.113.11");
        insert_sample_deployment(&conn, "deploy-newer", "newer-host", "203.0.113.12");

        conn.execute(
            "UPDATE deployments SET created_at = ?1 WHERE id = ?2",
            rusqlite::params!["2026-01-01 00:00:00", "deploy-older"],
        )
        .unwrap();
        conn.execute(
            "UPDATE deployments SET created_at = ?1 WHERE id = ?2",
            rusqlite::params!["2026-01-02 00:00:00", "deploy-newer"],
        )
        .unwrap();

        let found = find_deployment_by_query(&conn, "").unwrap().unwrap();
        assert_eq!(found.id, "deploy-newer");
    }

    #[test]
    fn delete_deployment_removes_existing_row() {
        let conn = test_conn();
        insert_sample_deployment(&conn, "deploy-1", "openclaw-one", "203.0.113.10");

        assert!(delete_deployment(&conn, "deploy-1").unwrap());
        assert!(get_deployment_by_id(&conn, "deploy-1").unwrap().is_none());
    }
}
