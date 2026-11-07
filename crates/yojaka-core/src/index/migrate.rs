//! `PRAGMA user_version` based migration runner.

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
