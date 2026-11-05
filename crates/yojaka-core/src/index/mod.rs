//! SQLite index. I0 only has the connection setup and the migration runner.

mod migrate;

use std::path::Path;

use rusqlite::Connection;

use crate::{Result, YojakaError};

#[derive(Debug)]
pub struct Index {
    conn: Connection,
}

impl Index {
    pub fn open(path: &Path) -> Result<Self> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| YojakaError::io(parent, e))?;
        }
        let conn = Connection::open(path)?;
        Self::init(conn, true)
    }

    pub fn open_in_memory() -> Result<Self> {
        Self::init(Connection::open_in_memory()?, false)
    }

    fn init(mut conn: Connection, wal: bool) -> Result<Self> {
        if wal {
            conn.pragma_update(None, "journal_mode", "WAL")?;
        }
        conn.pragma_update(None, "synchronous", "NORMAL")?;
        conn.pragma_update(None, "foreign_keys", "ON")?;
        conn.busy_timeout(std::time::Duration::from_millis(5000))?;
        migrate::run(&mut conn, migrate::MIGRATIONS)?;
        Ok(Self { conn })
    }

    pub fn schema_version(&self) -> Result<u32> {
        Ok(self
            .conn
            .query_row("PRAGMA user_version", [], |r| r.get(0))?)
    }

    pub fn connection(&self) -> &Connection {
        &self.conn
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fresh_database_is_at_version_1() {
        let idx = Index::open_in_memory().unwrap();
        assert_eq!(idx.schema_version().unwrap(), 1);
        let v: String = idx
            .connection()
            .query_row(
                "SELECT value FROM meta_kv WHERE key='created_by'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(v, "yojaka");
    }

    #[test]
    fn reopening_is_a_noop() {
        let tmp = tempfile::tempdir().unwrap();
        let p = tmp.path().join("i.db");
        drop(Index::open(&p).unwrap());
        let idx = Index::open(&p).unwrap();
        assert_eq!(idx.schema_version().unwrap(), 1);
    }

    #[test]
    fn newer_index_is_rejected_clearly() {
        let tmp = tempfile::tempdir().unwrap();
        let p = tmp.path().join("i.db");
        Connection::open(&p)
            .unwrap()
            .pragma_update(None, "user_version", 99)
            .unwrap();
        let err = Index::open(&p).unwrap_err();
        assert!(
            err.to_string().contains("created by a newer Yojaka"),
            "{err}"
        );
    }
}
