//! The core → UI event channel.

use std::path::PathBuf;

#[derive(Debug, Clone)]
pub enum CoreEvent {
    TaskStarted {
        task: TaskKind,
        label: String,
    },
    Progress {
        task: TaskKind,
        done: u64,
        total: Option<u64>,
        bytes: u64,
        current: Option<PathBuf>,
    },
    Issue(crate::FileIssue),
    TaskFinished {
        task: TaskKind,
        outcome: TaskOutcome,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TaskKind {
    Scan,
    Metadata,
    Geocode,
    Hash,
    Plan,
    Execute,
    Undo,
}

#[derive(Debug, Clone)]
pub enum TaskOutcome {
    Complete,
    Partial,
    Cancelled,
    Failed(String),
}

/// Wraps a bounded sender.
/// - [`progress`](Self::progress) never blocks; it drops the event if the UI is behind.
/// - [`important`](Self::important) blocks; used for start/finish/issue events.
#[derive(Debug, Clone)]
pub struct EventSink(crossbeam_channel::Sender<CoreEvent>);

impl EventSink {
    pub fn channel(cap: usize) -> (Self, crossbeam_channel::Receiver<CoreEvent>) {
        let (tx, rx) = crossbeam_channel::bounded(cap);
        (Self(tx), rx)
    }
    pub fn progress(&self, ev: CoreEvent) {
        let _ = self.0.try_send(ev);
    }
    pub fn important(&self, ev: CoreEvent) {
        let _ = self.0.send(ev);
    }
    /// A sink whose receiver is gone. Used by `--quiet` and tests.
    pub fn null() -> Self {
        Self::channel(1).0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn progress() -> CoreEvent {
        CoreEvent::Progress {
            task: TaskKind::Scan,
            done: 1,
            total: None,
            bytes: 0,
            current: None,
        }
    }

    #[test]
    fn progress_drops_when_full() {
        let (sink, rx) = EventSink::channel(1);
        sink.progress(progress());
        sink.progress(progress());
        assert_eq!(rx.len(), 1);
    }

    #[test]
    fn null_sink_does_not_block_or_panic() {
        let sink = EventSink::null();
        sink.progress(progress());
        sink.important(progress());
    }
}
