//! Runs work on a background thread so the UI never freezes.
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::mpsc::{Receiver, channel};

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
            let _ = tx.send(f(p, c));
            ctx.request_repaint();
        });
        Self { rx, progress, cancel }
    }

    pub fn try_take(&self) -> Option<T> {
        self.rx.try_recv().ok()
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
        Some(r) => {
            *slot = None;
            Some(r)
        }
        None => {
            ctx.request_repaint_after(std::time::Duration::from_millis(200));
            None
        }
    }
}
