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
    let total: u32 = conn
        .query_row("SELECT COUNT(*) FROM deployments", [], |row| row.get(0))?;
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
    let changed = conn.execute("DELETE FROM deployments WHERE id = ?1", rusqlite::params![id])?;
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
