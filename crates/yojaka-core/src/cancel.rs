//! Cooperative cancellation shared between the UI and worker threads.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

#[derive(Debug, Clone, Default)]
pub struct CancelToken(Arc<AtomicBool>);

impl CancelToken {
    pub fn new() -> Self {
        Self::default()
    }
    pub fn cancel(&self) {
        self.0.store(true, Ordering::Relaxed)
    }
    pub fn is_cancelled(&self) -> bool {
        self.0.load(Ordering::Relaxed)
    }
    /// The raw flag, for APIs that poll an `AtomicBool` (the directory walker).
    pub fn flag(&self) -> &AtomicBool {
        &self.0
    }
    /// `?`-friendly check inside loops.
    pub fn check(&self) -> crate::Result<()> {
        if self.is_cancelled() {
            Err(crate::YojakaError::Cancelled)
        } else {
            Ok(())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cancel_is_shared_between_clones() {
        let a = CancelToken::new();
        let b = a.clone();
        assert!(a.check().is_ok());
        b.cancel();
        assert!(a.is_cancelled());
        assert!(matches!(a.check(), Err(crate::YojakaError::Cancelled)));
    }
}
