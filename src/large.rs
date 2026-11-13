//! Large & old files in the home folder, grouped by kind.
use crate::fsutil::{self, fmt_age, fmt_size};
use crate::job::{self, Job};
use crate::junk::{busy_screen, done_screen};
use crate::theme::{self, Palette, icon};
use eframe::egui::{self, Color32, Layout, Ui};
use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::time::{Duration, SystemTime};

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Video,
    Archive,
    Installer,
    Model,
    Audio,
    Image,
    Document,
    Other,
}

impl Kind {
    const ALL: [Kind; 8] = [
        Kind::Video,
        Kind::Archive,
        Kind::Installer,
        Kind::Model,
        Kind::Audio,
        Kind::Image,
        Kind::Document,
        Kind::Other,
    ];

    fn of(p: &std::path::Path) -> Kind {
        let ext = p.extension().map(|e| e.to_string_lossy().to_lowercase()).unwrap_or_default();
        match ext.as_str() {
            "mp4" | "mov" | "mkv" | "avi" | "m4v" | "webm" | "wmv" | "flv" => Kind::Video,
            "zip" | "rar" | "7z" | "tar" | "gz" | "tgz" | "bz2" | "xz" | "zst" => Kind::Archive,
            "dmg" | "iso" | "pkg" | "img" | "ipa" | "xip" | "app" => Kind::Installer,
            "pt" | "pth" | "ckpt" | "safetensors" | "onnx" | "h5" | "npy" | "npz" | "parquet" | "gguf" | "bin" | "tflite"
            | "pkl" | "db" | "sqlite" => Kind::Model,
            "mp3" | "wav" | "flac" | "aac" | "m4a" | "aiff" | "ogg" => Kind::Audio,
            "jpg" | "jpeg" | "png" | "heic" | "tif" | "tiff" | "psd" | "raw" | "cr2" | "nef" | "arw" | "gif" | "webp" => {
                Kind::Image
            }
            "pdf" | "doc" | "docx" | "ppt" | "pptx" | "xls" | "xlsx" | "key" | "pages" | "numbers" | "epub" | "txt" | "csv"
            | "json" => Kind::Document,
            _ => Kind::Other,
        }
    }
    fn label(self) -> &'static str {
        match self {
            Kind::Video => "Videos",
            Kind::Archive => "Archives",
            Kind::Installer => "Installers & Disk Images",
            Kind::Model => "Models & Data",
            Kind::Audio => "Audio",
            Kind::Image => "Images",
            Kind::Document => "Documents",
            Kind::Other => "Other",
        }
    }
    fn glyph(self) -> &'static str {
        match self {
            Kind::Video => icon::FILE_VIDEO,
            Kind::Archive => icon::FILE_ZIP,
            Kind::Installer => icon::DISC,
            Kind::Model => icon::BRAIN,
            Kind::Audio => icon::FILE_AUDIO,
            Kind::Image => icon::FILE_IMAGE,
            Kind::Document => icon::FILE_TEXT,
            Kind::Other => icon::FILE,
        }
    }
    fn color(self) -> Color32 {
        let c = Color32::from_rgb;
        match self {
            Kind::Video => c(0xe2, 0x4a, 0x6a),
            Kind::Archive => c(0xc9, 0x8a, 0x2a),
            Kind::Installer => c(0x6a, 0x6f, 0xe0),
            Kind::Model => c(0xd9, 0x6c, 0x2b),
            Kind::Audio => c(0x2f, 0xb5, 0x7c),
            Kind::Image => c(0x2b, 0x9c, 0xc4),
            Kind::Document => c(0x4f, 0x86, 0xe8),
            Kind::Other => c(0x7d, 0x7d, 0x8c),
        }
    }
}

pub struct LargeFile {
    path: PathBuf,
    size: u64,
    last_used: Option<SystemTime>,
    kind: Kind,
}

#[derive(PartialEq, Clone, Copy)]
enum SortBy {
    Size,
    Age,
    Name,
}

pub struct Large {
    files: Vec<LargeFile>,
    scanned: bool,
    selected: HashSet<PathBuf>,
    min_mb: u64,
    filter: Option<Kind>,
    old_only: bool,
    sort: SortBy,
    scan: Option<Job<Vec<LargeFile>>>,
    trash: Option<Job<Result<(), String>>>,
    done: Option<(usize, u64, Option<String>)>,
}

impl Default for Large {
    fn default() -> Self {
        Self {
            files: Vec::new(),
            scanned: false,
            selected: HashSet::new(),
            min_mb: 100,
            filter: None,
            old_only: false,
            sort: SortBy::Size,
            scan: None,
            trash: None,
            done: None,
        }
    }
}

fn walk(min_bytes: u64, progress: Arc<AtomicUsize>, cancel: Arc<AtomicBool>) -> Vec<LargeFile> {
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
    let mut found = Vec::new();
    for entry in walker.into_iter().filter_map(Result::ok) {
        progress.fetch_add(1, Ordering::Relaxed);
        if !entry.file_type.is_file() {
            continue;
        }
        let Ok(md) = entry.metadata() else { continue };
        let size = fsutil::allocated(&md);
        if size >= min_bytes {
            let path = entry.path();
            let last_used = md.accessed().ok().or_else(|| md.modified().ok());
            found.push(LargeFile {
                kind: Kind::of(&path),
                path,
                size,
                last_used,
            });
        }
    }
    found
}

fn is_old(t: Option<SystemTime>) -> bool {
    t.and_then(|t| SystemTime::now().duration_since(t).ok())
        .is_some_and(|d| d > Duration::from_secs(365 * 86_400))
}

impl Large {
    fn sort(&mut self) {
        match self.sort {
            SortBy::Size => self.files.sort_unstable_by(|a, b| b.size.cmp(&a.size)),
            SortBy::Age => self.files.sort_unstable_by(|a, b| a.last_used.cmp(&b.last_used)),
            SortBy::Name => self
                .files
                .sort_unstable_by(|a, b| a.path.file_name().cmp(&b.path.file_name())),
        }
    }

    fn start(&mut self, ctx: &egui::Context) {
        self.selected.clear();
        self.done = None;
        let min = self.min_mb * 1_000_000;
        self.scan = Some(Job::spawn(ctx, move |p, c| walk(min, p, c)));
    }

    fn visible(&self, f: &LargeFile) -> bool {
        self.filter.is_none_or(|k| f.kind == k) && (!self.old_only || is_old(f.last_used))
    }

    pub fn ui(&mut self, ui: &mut Ui, p: Palette) {
        let ctx = ui.ctx().clone();
        if let Some(files) = job::poll(&mut self.scan, &ctx) {
            self.files = files;
            self.scanned = true;
            self.sort();
        }
        if let Some(res) = job::poll(&mut self.trash, &ctx) {
            let before = self.files.len();
            let freed: u64 = self.files.iter().filter(|f| !f.path.exists()).map(|f| f.size).sum();
            self.files.retain(|f| f.path.exists());
            self.selected.retain(|p| p.exists());
            self.done = Some((before - self.files.len(), freed, res.err()));
        }

        if let Some(job) = &self.scan {
            busy_screen(
                ui,
                p,
                icon::MAGNIFYING_GLASS,
                "Searching your home folder…",
                &format!("{} items checked", job.progress()),
            );
            theme::centered(ui, |ui| {
                ui.add_space(14.0);
                if theme::pill(ui, format!("{}  Stop", icon::STOP), Color32::from_white_alpha(40)).clicked() {
                    job.stop();
                }
            });
            return;
        }
        if self.trash.is_some() {
            busy_screen(ui, p, icon::TRASH, "Moving to Trash…", "Finder may ask for your password.");
            return;
        }
        if let Some((n, freed, err)) = &self.done {
            let sub = match err {
                Some(e) => format!("Some files couldn't be moved: {e}"),
                None => "Empty the Trash (bottom-left) to get the space back.".into(),
            };
            if done_screen(ui, p, &format!("{n} files · {} moved to Trash", fmt_size(*freed)), &sub) {
                self.done = None;
            }
            return;
        }

        if !self.scanned {
            theme::centered(ui, |ui| {
                ui.add_space(ui.available_height() * 0.10);
                theme::hero(ui, icon::FOLDER, p, 150.0, false, None);
                ui.add_space(10.0);
                ui.label(theme::bold("Large & Old Files", 30.0));
                ui.label(theme::dim(
                    "Find big files you forgot about — old downloads,\nvideos, disk images and model checkpoints.",
                    15.0,
                ));
                ui.add_space(16.0);
                ui.horizontal(|ui| {
                    let w = 4.0 * 82.0;
                    ui.add_space((ui.available_width() - w) / 2.0);
                    for mb in [50, 100, 500, 1000] {
                        let label = if mb >= 1000 {
                            "1 GB+".to_string()
                        } else {
                            format!("{mb} MB+")
                        };
                        let fill = if self.min_mb == mb {
                            Color32::from_white_alpha(80)
                        } else {
                            Color32::from_white_alpha(22)
                        };
                        if theme::pill(ui, label, fill).clicked() {
                            self.min_mb = mb;
                        }
                    }
                });
                ui.add_space(16.0);
                if theme::round_button(ui, "Scan", p, true).clicked() {
                    self.start(&ctx);
                }
            });
            return;
        }

        self.results(ui, p, &ctx);
    }

    fn results(&mut self, ui: &mut Ui, p: Palette, ctx: &egui::Context) {
        let total: u64 = self.files.iter().map(|f| f.size).sum();
        ui.horizontal(|ui| {
            theme::icon_tile(ui, icon::FOLDER, p.accent, 46.0);
            ui.vertical(|ui| {
                ui.label(theme::bold(fmt_size(total), 28.0));
                ui.label(theme::dim(
                    format!("in {} files larger than {} MB", self.files.len(), self.min_mb),
                    13.5,
                ));
            });
            ui.with_layout(Layout::right_to_left(egui::Align::Center), |ui| {
                if theme::pill(
                    ui,
                    format!("{}  Scan again", icon::ARROW_CLOCKWISE),
                    Color32::from_white_alpha(35),
                )
                .clicked()
                {
                    self.start(ctx);
                }
            });
        });
        ui.add_space(10.0);

        egui::Panel::bottom("large_action").frame(egui::Frame::new()).show(ui, |ui| {
            theme::centered(ui, |ui| {
                ui.add_space(4.0);
                let sel: u64 = self
                    .files
                    .iter()
                    .filter(|f| self.selected.contains(&f.path))
                    .map(|f| f.size)
                    .sum();
                if theme::round_button(ui, "Remove", p, !self.selected.is_empty()).clicked() {
                    let paths: Vec<PathBuf> = self.selected.iter().cloned().collect();
                    self.trash = Some(Job::spawn(ctx, move |_, _| fsutil::move_to_trash(&paths)));
                }
                ui.label(theme::dim(
                    format!("{} files · {} selected · moved to Trash", self.selected.len(), fmt_size(sel)),
                    12.5,
                ));
            });
        });

        // Kind filters
        egui::Panel::left("large_kinds")
            .resizable(false)
            .exact_size(250.0)
            .frame(egui::Frame::new())
            .show(ui, |ui| {
                let mut entry =
                    |ui: &mut Ui, k: Option<Kind>, label: &str, glyph: &str, color: Color32, size: u64, count: usize| {
                        let fill = if self.filter == k {
                            Color32::from_white_alpha(40)
                        } else {
                            Color32::from_white_alpha(12)
                        };
                        let r = egui::Frame::new()
                            .fill(fill)
                            .corner_radius(12)
                            .inner_margin(8)
                            .show(ui, |ui| {
                                ui.set_width(ui.available_width());
                                ui.horizontal(|ui| {
                                    theme::icon_tile(ui, glyph, color, 28.0);
                                    ui.vertical(|ui| {
                                        ui.label(theme::semi(label, 13.0));
                                        ui.label(theme::dim(format!("{count} files"), 11.0));
                                    });
                                    ui.with_layout(Layout::right_to_left(egui::Align::Center), |ui| {
                                        ui.label(theme::semi(fmt_size(size), 12.5))
                                    });
                                });
                            });
                        if r.response.interact(egui::Sense::click()).clicked() {
                            self.filter = k;
                        }
                        ui.add_space(2.0);
                    };
                egui::ScrollArea::vertical().auto_shrink(false).show(ui, |ui| {
                    let all: u64 = self.files.iter().map(|f| f.size).sum();
                    entry(ui, None, "All Files", icon::FOLDER, p.accent, all, self.files.len());
                    for k in Kind::ALL {
                        let (n, s) = self
                            .files
                            .iter()
                            .filter(|f| f.kind == k)
                            .fold((0, 0), |(n, s), f| (n + 1, s + f.size));
                        if n > 0 {
                            entry(ui, Some(k), k.label(), k.glyph(), k.color(), s, n);
                        }
                    }
                    ui.add_space(8.0);
                    ui.checkbox(&mut self.old_only, theme::text("Not opened in a year", 13.0));
                });
            });

        // File list
        theme::card(ui, |ui| {
            ui.set_min_size(ui.available_size());
            ui.horizontal(|ui| {
                ui.label(theme::dim("Sort by", 12.5));
                for (label, key) in [("Size", SortBy::Size), ("Oldest", SortBy::Age), ("Name", SortBy::Name)] {
                    let fill = if self.sort == key {
                        Color32::from_white_alpha(70)
                    } else {
                        Color32::from_white_alpha(18)
                    };
                    if theme::pill(ui, label, fill).clicked() {
                        self.sort = key;
                        self.sort();
                    }
                }
                ui.with_layout(Layout::right_to_left(egui::Align::Center), |ui| {
                    if !self.selected.is_empty() && ui.link(theme::dim("Deselect all", 12.5)).clicked() {
                        self.selected.clear();
                    }
                });
            });
            ui.separator();
            let idx: Vec<usize> = (0..self.files.len()).filter(|&i| self.visible(&self.files[i])).collect();
            if idx.is_empty() {
                theme::centered(ui, |ui| {
                    ui.add_space(60.0);
                    ui.label(theme::ic(icon::CHECK_CIRCLE, 44.0));
                    ui.label(theme::dim("Nothing here.", 14.0));
                });
                return;
            }
            let row_h = 46.0;
            egui::ScrollArea::vertical()
                .auto_shrink(false)
                .show_rows(ui, row_h, idx.len(), |ui, range| {
                    for &i in &idx[range] {
                        let f = &self.files[i];
                        ui.horizontal(|ui| {
                            ui.set_height(row_h);
                            let mut on = self.selected.contains(&f.path);
                            if ui.checkbox(&mut on, "").changed() {
                                if on {
                                    self.selected.insert(f.path.clone());
                                } else {
                                    self.selected.remove(&f.path);
                                }
                            }
                            theme::icon_tile(ui, f.kind.glyph(), f.kind.color(), 30.0);
                            ui.with_layout(Layout::right_to_left(egui::Align::Center), |ui| {
                                ui.add_space(6.0);
                                ui.add_sized([80.0, row_h], egui::Label::new(theme::semi(fmt_size(f.size), 13.5)));
                                let age = fmt_age(f.last_used);
                                let age_txt = if is_old(f.last_used) {
                                    theme::text(age, 12.5).color(theme::WARN)
                                } else {
                                    theme::dim(age, 12.5)
                                };
                                ui.add_sized([95.0, row_h], egui::Label::new(age_txt));
                                ui.with_layout(Layout::top_down(egui::Align::Min), |ui| {
                                    ui.add_space(5.0);
                                    let name = f
                                        .path
                                        .file_name()
                                        .map(|n| n.to_string_lossy().into_owned())
                                        .unwrap_or_default();
                                    ui.add(egui::Label::new(theme::text(name, 13.5)).truncate());
                                    let loc = f.path.parent().map(fsutil::pretty).unwrap_or_default();
                                    let r =
                                        ui.add(egui::Label::new(theme::dim(loc, 11.5)).truncate().sense(egui::Sense::click()));
                                    if r.on_hover_text("Reveal in Finder").clicked() {
                                        fsutil::reveal(&f.path);
                                    }
                                });
                            });
                        });
                    }
                });
        });
    }
}
