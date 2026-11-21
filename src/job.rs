//! Runs work on a background thread so the UI never freezes.
use std::sync::Arc;
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::mpsc::{Receiver, TryRecvError, channel};

/// A task that crashed, waiting to be shown to the user.
static FAILED: Mutex<Option<String>> = Mutex::new(None);

/// Takes the last crash message, if any.
pub fn take_failure() -> Option<String> {
    FAILED.lock().ok()?.take()
}

pub struct Job<T> {
    rx: Receiver<T>,
    pub progress: Arc<AtomicUsize>,
    pub cancel: Arc<AtomicBool>,
}

impl<T: Send + 'static> Job<T> {
    pub fn spawn<F>(ctx: &eframe::egui::Context, f: F) -> Self
    where
        F: FnOnce(Arc<AtomicUsize>, Arc<AtomicBool>) -> T + Send + 'static,
    {
        let (tx, rx) = channel();
        let progress = Arc::new(AtomicUsize::new(0));
        let cancel = Arc::new(AtomicBool::new(false));
        let (p, c, ctx) = (progress.clone(), cancel.clone(), ctx.clone());
        std::thread::spawn(move || {
            // A bug in one task must not close the app: report it instead.
            match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| f(p, c))) {
                Ok(v) => {
                    let _ = tx.send(v);
                }
                Err(e) => {
                    let why = e
                        .downcast_ref::<&str>()
                        .map(|s| s.to_string())
                        .or_else(|| e.downcast_ref::<String>().cloned())
                        .unwrap_or_else(|| "unknown error".into());
                    if let Ok(mut f) = FAILED.lock() {
                        *f = Some(why);
                    }
                }
            }
            ctx.request_repaint();
        });
        Self { rx, progress, cancel }
    }

    pub fn try_take(&self) -> Result<T, TryRecvError> {
        self.rx.try_recv()
    }

    pub fn progress(&self) -> usize {
        self.progress.load(Ordering::Relaxed)
    }

    pub fn stop(&self) {
        self.cancel.store(true, Ordering::Relaxed);
    }
}

/// Polls an optional job; returns the result once and clears the slot.
/// While running, schedules a light repaint so progress text updates.
pub fn poll<T: Send + 'static>(slot: &mut Option<Job<T>>, ctx: &eframe::egui::Context) -> Option<T> {
    let job = slot.as_ref()?;
    match job.try_take() {
        Ok(r) => {
            *slot = None;
            Some(r)
        }
        // The task crashed: stop waiting for it (the error is shown separately).
        Err(TryRecvError::Disconnected) => {
            *slot = None;
            None
        }
        Err(TryRecvError::Empty) => {
            ctx.request_repaint_after(std::time::Duration::from_millis(200));
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_crashing_task_is_reported_not_fatal() {
        let ctx = eframe::egui::Context::default();
        let mut slot: Option<Job<u32>> = Some(Job::spawn(&ctx, |_, _| -> u32 { panic!("boom") }));
        for _ in 0..1000 {
            if slot.is_none() {
                break;
            }
            assert!(poll(&mut slot, &ctx).is_none());
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        assert!(slot.is_none(), "the job slot is cleared");
        assert_eq!(take_failure().as_deref(), Some("boom"));
    }
}
