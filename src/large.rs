//! Large & old files in the home folder.
use crate::fsutil::{self, fmt_age, fmt_size};
use crate::job::{self, Job};
use eframe::egui::{self, RichText, Ui};
use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::SystemTime;

pub struct LargeFile {
    path: PathBuf,
    size: u64,
    last_used: Option<SystemTime>,
}

#[derive(PartialEq, Clone, Copy)]
enum SortBy {
    Size,
    Age,
    Name,
}

pub struct Large {
    files: Vec<LargeFile>,
    selected: HashSet<PathBuf>,
    min_mb: u64,
    sort: SortBy,
    scan: Option<Job<Vec<LargeFile>>>,
    trash: Option<Job<Result<(), String>>>,
    status: String,
}

impl Default for Large {
    fn default() -> Self {
        Self {
            files: Vec::new(),
            selected: HashSet::new(),
            min_mb: 100,
            sort: SortBy::Size,
            scan: None,
            trash: None,
            status: String::new(),
        }
    }
}

fn walk(min_bytes: u64, job_progress: Arc<AtomicUsize>, cancel: Arc<AtomicBool>) -> Vec<LargeFile> {
    let mut found = Vec::new();
    let cancel_cb = cancel.clone();
    let walker = jwalk::WalkDir::new(fsutil::home())
        .skip_hidden(false)
        .follow_links(false)
        .process_read_dir(move |_, _, _, children| {
            if cancel_cb.load(Ordering::Relaxed) {
                children.clear();
                return;
            }
            // Treat app bundles as one item — the Uninstaller handles them.
            for e in children.iter_mut().flatten() {
                if e.file_type.is_dir() && e.file_name.to_string_lossy().ends_with(".app") {
                    e.read_children = None;
                }
            }
        });
    for entry in walker.into_iter().filter_map(Result::ok) {
        job_progress.fetch_add(1, Ordering::Relaxed);
        if !entry.file_type.is_file() {
            continue;
        }
        let Ok(md) = entry.metadata() else { continue };
        let size = fsutil::allocated(&md);
        if size >= min_bytes {
            let last_used = md.accessed().ok().or_else(|| md.modified().ok());
            found.push(LargeFile { path: entry.path(), size, last_used });
        }
    }
    found
}

/// Fixed-width, left-aligned table cell.
fn cell(ui: &mut Ui, w: f32, h: f32, add: impl FnOnce(&mut Ui)) {
    ui.allocate_ui_with_layout(egui::vec2(w, h), egui::Layout::left_to_right(egui::Align::Center), |ui| {
        ui.set_min_width(w);
        add(ui);
    });
}

impl Large {
    fn sort(&mut self) {
        match self.sort {
            SortBy::Size => self.files.sort_unstable_by(|a, b| b.size.cmp(&a.size)),
            SortBy::Age => self.files.sort_unstable_by(|a, b| a.last_used.cmp(&b.last_used)),
            SortBy::Name => self.files.sort_unstable_by(|a, b| a.path.file_name().cmp(&b.path.file_name())),
        }
    }

    pub fn ui(&mut self, ui: &mut Ui) {
        let ctx = ui.ctx().clone();
        if let Some(files) = job::poll(&mut self.scan, &ctx) {
            self.files = files;
            self.sort();
        }
        if let Some(res) = job::poll(&mut self.trash, &ctx) {
            let before = self.files.len();
            self.files.retain(|f| f.path.exists());
            self.selected.retain(|p| p.exists());
            self.status = match res {
                Ok(()) => format!("Moved {} file(s) to the Trash", before - self.files.len()),
                Err(e) => format!("Couldn't move some files: {e}"),
            };
        }

        ui.horizontal(|ui| {
            ui.heading("Large & Old Files");
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if let Some(job) = &self.scan {
                    if ui.button("Stop").clicked() {
                        job.stop();
                    }
                    ui.label(format!("{} items checked", job.progress()));
                    ui.spinner();
                } else {
                    if ui.button("Scan Home Folder").clicked() {
                        self.selected.clear();
                        self.status.clear();
                        let min = self.min_mb * 1_000_000;
                        self.scan = Some(Job::spawn(&ctx, move |p, c| walk(min, p, c)));
                    }
                    egui::ComboBox::from_id_salt("min")
                        .selected_text(format!("Bigger than {}", if self.min_mb >= 1000 { "1 GB".into() } else { format!("{} MB", self.min_mb) }))
                        .show_ui(ui, |ui| {
                            for mb in [50, 100, 500, 1000] {
                                let label = if mb >= 1000 { "1 GB".to_string() } else { format!("{mb} MB") };
                                ui.selectable_value(&mut self.min_mb, mb, label);
                            }
                        });
                }
            });
        });
        ui.separator();

        egui::Panel::bottom("large_bottom").show(ui, |ui| {
            ui.add_space(6.0);
            ui.horizontal(|ui| {
                ui.label(&self.status);
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    let busy = self.trash.is_some();
                    if ui.add_enabled(!self.selected.is_empty() && !busy, egui::Button::new("Move to Trash")).clicked() {
                        let paths: Vec<PathBuf> = self.selected.iter().cloned().collect();
                        self.trash = Some(Job::spawn(&ctx, move |_, _| fsutil::move_to_trash(&paths)));
                    }
                    let sel: u64 = self.files.iter().filter(|f| self.selected.contains(&f.path)).map(|f| f.size).sum();
                    ui.label(format!("{} selected · {}", self.selected.len(), fmt_size(sel)));
                });
            });
            ui.add_space(4.0);
        });

        // Column headers (click to sort)
        let widths = [24.0, 260.0, 90.0, 110.0];
        ui.horizontal(|ui| {
            ui.add_space(widths[0]);
            for (label, key, w) in [("Name", SortBy::Name, widths[1]), ("Size", SortBy::Size, widths[2]), ("Last opened", SortBy::Age, widths[3])] {
                let mut text = RichText::new(label).strong();
                if self.sort == key {
                    text = text.color(ui.visuals().selection.bg_fill).underline();
                }
                let mut clicked = false;
                cell(ui, w, 18.0, |ui| clicked = ui.add(egui::Button::new(text).frame(false)).on_hover_text("Sort").clicked());
                if clicked {
                    self.sort = key;
                    self.sort();
                }
            }
            ui.label(RichText::new("Location").strong());
        });
        ui.separator();

        // Virtualised list: only visible rows are laid out, so 100k results stay cheap.
        let row_h = ui.text_style_height(&egui::TextStyle::Body) + 6.0;
        egui::ScrollArea::vertical().auto_shrink(false).show_rows(ui, row_h, self.files.len(), |ui, range| {
            for f in &self.files[range] {
                ui.horizontal(|ui| {
                    let mut on = self.selected.contains(&f.path);
                    if ui.checkbox(&mut on, "").changed() {
                        if on {
                            self.selected.insert(f.path.clone());
                        } else {
                            self.selected.remove(&f.path);
                        }
                    }
                    let name = f.path.file_name().map(|n| n.to_string_lossy()).unwrap_or_default();
                    cell(ui, widths[1], row_h, |ui| { ui.add(egui::Label::new(name).truncate()); });
                    cell(ui, widths[2], row_h, |ui| { ui.label(fmt_size(f.size)); });
                    cell(ui, widths[3], row_h, |ui| { ui.label(fmt_age(f.last_used)); });
                    let loc = f.path.parent().map(fsutil::pretty).unwrap_or_default();
                    if ui.add(egui::Label::new(RichText::new(loc).weak()).truncate().sense(egui::Sense::click()))
                        .on_hover_text("Reveal in Finder")
                        .clicked()
                    {
                        fsutil::reveal(&f.path);
                    }
                });
            }
        });
    }
}
