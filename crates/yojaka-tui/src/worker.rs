//! Background threads that read the index, so the UI thread never touches SQLite.
//!
//! Two workers: a *fast* one for the file table (counts, pages, one file's details) and a *slow*
//! one for whole-index work (statistics, folder tree). A slow statistics query therefore never
//! delays scrolling. Each worker drops requests that a newer one makes obsolete.

use std::path::PathBuf;
use std::sync::Arc;
use std::thread::JoinHandle;
use std::time::Instant;

use crossbeam_channel::{Receiver, Sender};
use yojaka_core::browse::{BrowseQuery, Browser, FileDetail, FileRow, FolderTree};
use yojaka_core::stats::Stats;
use yojaka_core::{Engine, YojakaError};

#[derive(Debug, Clone)]
pub enum Request {
    /// Forget the open index handle (after a scan, or when the index file was replaced).
    Reload,
    Stats,
    Folders,
    Count {
        generation: u64,
        query: BrowseQuery,
    },
    Page {
        generation: u64,
        query: BrowseQuery,
        offset: u64,
        limit: u64,
    },
    Detail {
        id: i64,
    },
}

impl Request {
    /// Whole-index work goes to the slow worker.
    pub fn is_slow(&self) -> bool {
        matches!(self, Request::Stats | Request::Folders)
    }

    fn slot(&self) -> u8 {
        match self {
            Request::Reload => 0,
            Request::Stats => 1,
            Request::Folders => 2,
            Request::Count { .. } => 3,
            Request::Page { .. } => 4,
            Request::Detail { .. } => 5,
        }
    }
}

#[derive(Debug)]
pub enum Response {
    /// The folder has no index yet.
    NoIndex,
    /// The index cannot be opened or read (protected folder, newer index version, ...).
    Error(String),
    Stats(Box<Stats>),
    Folders(FolderTree),
    Count {
        generation: u64,
        total: u64,
    },
    Page {
        generation: u64,
        offset: u64,
        limit: u64,
        rows: Vec<FileRow>,
        ms: f64,
    },
    Detail {
        id: i64,
        detail: Option<Box<FileDetail>>,
    },
}

/// What the UI thread receives from every background thread.
#[derive(Debug)]
pub enum Bg {
    Worker(Response),
    ScanFinished(Result<yojaka_core::scan::ScanSummary, String>),
}

/// Keeps only the newest request of each kind, in their original order. `Reload` is always kept.
pub fn coalesce(batch: Vec<Request>) -> Vec<Request> {
    let mut seen = [false; 6];
    let mut kept: Vec<Request> = batch
        .into_iter()
        .rev()
        .filter(|r| {
            let s = r.slot() as usize;
            if s == 0 {
                return true;
            }
            !std::mem::replace(&mut seen[s], true)
        })
        .collect();
    kept.reverse();
    kept
}

struct State {
    engine: Arc<Engine>,
    root: PathBuf,
    local: bool,
    browser: Option<Browser>,
}

impl State {
    fn browser(&mut self) -> Result<&Browser, Response> {
        if self.browser.is_none() {
            match self.engine.browser(&self.root, self.local) {
                Ok(b) => self.browser = Some(b),
                Err(e) => return Err(error_response(&e)),
            }
        }
        self.browser
            .as_ref()
            .ok_or_else(|| Response::Error("index handle missing".into()))
    }

    fn handle(&mut self, req: Request) -> Option<Response> {
        let resp = match req {
            Request::Reload => {
                self.browser = None;
                return None;
            }
            Request::Stats => match self.engine.stats(&self.root, self.local) {
                Ok(s) => Response::Stats(Box::new(s)),
                Err(e) => error_response(&e),
            },
            Request::Folders => match self
                .browser()
                .and_then(|b| b.folders().map_err(|e| Response::Error(e.to_string())))
            {
                Ok(t) => Response::Folders(t),
                Err(r) => r,
            },
            Request::Count { generation, query } => match self
                .browser()
                .and_then(|b| b.count(&query).map_err(|e| Response::Error(e.to_string())))
            {
                Ok(total) => Response::Count { generation, total },
                Err(r) => r,
            },
            Request::Page {
                generation,
                query,
                offset,
                limit,
            } => {
                let t = Instant::now();
                match self.browser().and_then(|b| {
                    b.page(&query, offset, limit)
                        .map_err(|e| Response::Error(e.to_string()))
                }) {
                    Ok(rows) => Response::Page {
                        generation,
                        offset,
                        limit,
                        rows,
                        ms: t.elapsed().as_secs_f64() * 1000.0,
                    },
                    Err(r) => r,
                }
            }
            Request::Detail { id } => match self
                .browser()
                .and_then(|b| b.detail(id).map_err(|e| Response::Error(e.to_string())))
            {
                Ok(d) => Response::Detail {
                    id,
                    detail: d.map(Box::new),
                },
                Err(r) => r,
            },
        };
        Some(resp)
    }
}

fn error_response(e: &YojakaError) -> Response {
    match e {
        YojakaError::NoIndex(_) => Response::NoIndex,
        other => Response::Error(other.to_string()),
    }
}

/// Starts one worker thread. It ends when `rx` is closed or the UI stops listening.
pub fn spawn(
    name: &str,
    engine: Arc<Engine>,
    root: PathBuf,
    local: bool,
    rx: Receiver<Request>,
    tx: Sender<Bg>,
) -> std::io::Result<JoinHandle<()>> {
    std::thread::Builder::new()
        .name(name.to_string())
        .spawn(move || {
            let mut st = State {
                engine,
                root,
                local,
                browser: None,
            };
            while let Ok(first) = rx.recv() {
                let mut batch = vec![first];
                batch.extend(rx.try_iter());
                for req in coalesce(batch) {
                    if let Some(resp) = st.handle(req)
                        && tx.send(Bg::Worker(resp)).is_err()
                    {
                        return;
                    }
                }
            }
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn page(generation: u64) -> Request {
        Request::Page {
            generation,
            query: BrowseQuery::default(),
            offset: 0,
            limit: 10,
        }
    }

    #[test]
    fn only_the_newest_request_of_each_kind_survives() {
        let out = coalesce(vec![
            page(1),
            Request::Detail { id: 1 },
            page(2),
            Request::Detail { id: 2 },
            Request::Stats,
            Request::Stats,
        ]);
        let shape: Vec<String> = out
            .iter()
            .map(|r| match r {
                Request::Page { generation, .. } => format!("page{generation}"),
                Request::Detail { id } => format!("detail{id}"),
                Request::Stats => "stats".into(),
                _ => "other".into(),
            })
            .collect();
        assert_eq!(shape, ["page2", "detail2", "stats"]);
    }

    #[test]
    fn reload_keeps_its_place_in_the_order() {
        let out = coalesce(vec![page(1), Request::Reload, page(2), Request::Reload]);
        assert!(matches!(out[0], Request::Reload));
        assert!(matches!(out[1], Request::Page { generation: 2, .. }));
        assert!(matches!(out[2], Request::Reload));
        assert_eq!(out.len(), 3);
    }

    #[test]
    fn stats_and_folders_are_slow_the_rest_fast() {
        assert!(Request::Stats.is_slow());
        assert!(Request::Folders.is_slow());
        assert!(!page(0).is_slow());
        assert!(!Request::Detail { id: 0 }.is_slow());
    }
}
