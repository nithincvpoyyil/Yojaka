//! The single DB writer thread: receives batches and writes each in one transaction.

use crossbeam_channel::Receiver;
use rusqlite::{Connection, params};

use crate::{FileIssue, IssueKind, Result};

#[derive(Debug)]
pub(crate) struct FileRecord {
    pub path: Vec<u8>,
    pub parent: Vec<u8>,
    pub name: Vec<u8>,
    pub ext: Option<String>,
    pub size: i64,
    pub mtime_ns: i64,
    pub ctime_ns: i64,
    pub btime_ns: Option<i64>,
    pub dev: i64,
    pub ino: i64,
    pub kind: &'static str,
    pub format: &'static str,
    pub cloud_only: bool,
}

#[derive(Debug)]
pub(crate) enum Item {
    /// New or changed file: insert, or replace the row and clear stale hashes.
    Upsert(FileRecord),
    /// Unchanged file: only refresh `last_seen_scan`.
    Touch(i64),
    Issue(FileIssue),
}

#[derive(Debug, Default)]
pub(crate) struct WriterStats {
    pub upserted: u64,
    pub touched: u64,
    pub issues: u64,
    pub permission_denied: u64,
}

const UPSERT: &str = "INSERT INTO file(full_path,parent_folder,file_name,extension,size_bytes,modified_at_ns,\
status_changed_at_ns,created_at_ns,device_id,inode,media_kind,format,is_cloud_only,\
last_seen_scan_id,metadata_status) \
VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,'pending') \
ON CONFLICT(full_path) DO UPDATE SET parent_folder=excluded.parent_folder,\
file_name=excluded.file_name,extension=excluded.extension,size_bytes=excluded.size_bytes,\
modified_at_ns=excluded.modified_at_ns,status_changed_at_ns=excluded.status_changed_at_ns,\
created_at_ns=excluded.created_at_ns,device_id=excluded.device_id,inode=excluded.inode,\
media_kind=excluded.media_kind,format=excluded.format,is_cloud_only=excluded.is_cloud_only,\
last_seen_scan_id=excluded.last_seen_scan_id,metadata_status='pending',partial_hash=NULL,\
full_hash=NULL,perceptual_hash=NULL,color_signature=NULL";

pub(crate) fn run(
    mut conn: Connection,
    rx: Receiver<Vec<Item>>,
    scan_id: i64,
) -> Result<(Connection, WriterStats)> {
    let mut stats = WriterStats::default();
    for batch in rx {
        let tx = conn.transaction()?;
        {
            let mut upsert = tx.prepare_cached(UPSERT)?;
            let mut touch =
                tx.prepare_cached("UPDATE file SET last_seen_scan_id=?1 WHERE id=?2")?;
            let mut issue = tx.prepare_cached(
                "INSERT INTO scan_error(scan_id,file_path,error_kind,error_message) VALUES(?1,?2,?3,?4)",
            )?;
            for item in batch {
                match item {
                    Item::Upsert(r) => {
                        upsert.execute(params![
                            r.path,
                            r.parent,
                            r.name,
                            r.ext,
                            r.size,
                            r.mtime_ns,
                            r.ctime_ns,
                            r.btime_ns,
                            r.dev,
                            r.ino,
                            r.kind,
                            r.format,
                            r.cloud_only,
                            scan_id
                        ])?;
                        stats.upserted += 1;
                    }
                    Item::Touch(id) => {
                        touch.execute(params![scan_id, id])?;
                        stats.touched += 1;
                    }
                    Item::Issue(i) => {
                        use std::os::unix::ffi::OsStrExt;
                        issue.execute(params![
                            scan_id,
                            i.path.as_os_str().as_bytes(),
                            format!("{:?}", i.kind),
                            i.message
                        ])?;
                        stats.issues += 1;
                        if i.kind == IssueKind::PermissionDenied {
                            stats.permission_denied += 1;
                        }
                    }
                }
            }
        }
        tx.commit()?;
    }
    Ok((conn, stats))
}
