//! `PRAGMA user_version` based migration runner.

use std::collections::HashSet;
use std::sync::OnceLock;

use rusqlite::Connection;

use crate::{Result, YojakaError};

pub(crate) const MIGRATIONS: &[(u32, &str)] = &[
    (1, include_str!("migrations/0001_init.sql")),
    (2, include_str!("migrations/0002_scan_file.sql")),
    (3, include_str!("migrations/0003_meta.sql")),
];

pub(crate) fn run(conn: &mut Connection, migrations: &[(u32, &str)]) -> Result<()> {
    let current: u32 = conn.query_row("PRAGMA user_version", [], |r| r.get(0))?;
    let latest = migrations.iter().map(|m| m.0).max().unwrap_or(0);
    if current > latest {
        return Err(YojakaError::IndexVersion(format!(
            "index was created by a newer Yojaka (schema {current}, this build knows {latest})"
        )));
    }
    for (version, sql) in migrations.iter().filter(|m| m.0 > current) {
        let tx = conn.transaction()?;
        tx.execute_batch(sql)?;
        tx.pragma_update(None, "user_version", version)?;
        tx.commit()?;
    }
    Ok(())
}

/// Until v1.0 a migration is changed in place (no fix-up migrations), so an index made by an
/// older build can be at the latest version and still lack a column. Say so plainly here,
/// instead of failing later with "no such column".
pub(crate) fn check_columns(conn: &Connection) -> Result<()> {
    let have: HashSet<(String, String)> = columns(conn)?.into_iter().collect();
    match fresh_columns()?.iter().find(|c| !have.contains(*c)) {
        None => Ok(()),
        Some((table, column)) => Err(YojakaError::IndexVersion(format!(
            "the saved data for this folder is from an older Yojaka build (it has no {table}.{column})"
        ))),
    }
}

/// Every `(table, column)` of a fresh index, worked out once from [`MIGRATIONS`].
fn fresh_columns() -> Result<&'static [(String, String)]> {
    static FRESH: OnceLock<Vec<(String, String)>> = OnceLock::new();
    if let Some(cols) = FRESH.get() {
        return Ok(cols);
    }
    let mut conn = Connection::open_in_memory()?;
    run(&mut conn, MIGRATIONS)?;
    let cols = columns(&conn)?;
    Ok(FRESH.get_or_init(|| cols))
}

fn columns(conn: &Connection) -> Result<Vec<(String, String)>> {
    let mut stmt = conn.prepare(
        "SELECT m.name, p.name FROM sqlite_master m JOIN pragma_table_info(m.name) p \
         WHERE m.type = 'table' AND m.name NOT LIKE 'sqlite_%' ORDER BY m.name, p.cid",
    )?;
    let rows = stmt.query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?;
    Ok(rows.collect::<rusqlite::Result<_>>()?)
}
