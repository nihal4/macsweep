//! System Junk: caches, logs, build data, Trash…
use crate::fsutil::{self, fmt_size};
use crate::job::{self, Job};
use eframe::egui::{self, RichText, Ui};
use rayon::prelude::*;
use std::path::PathBuf;

type Items = Vec<(PathBuf, u64)>;

pub struct Category {
    name: &'static str,
    detail: &'static str,
    roots: &'static [&'static str], // relative to ~
    pub selected: bool,
    items: Items,
    scanned: bool,
    expanded: bool,
}

impl Category {
    const fn new(name: &'static str, detail: &'static str, roots: &'static [&'static str], selected: bool) -> Self {
        Self { name, detail, roots, selected, items: Vec::new(), scanned: false, expanded: false }
    }
    fn total(&self) -> u64 {
        self.items.iter().map(|i| i.1).sum()
    }
}

pub struct Junk {
    cats: Vec<Category>,
    scan: Option<Job<Vec<Items>>>,
    clean: Option<Job<(u64, usize)>>,
    pub scanned: bool,
    confirm: bool,
    status: String,
}

impl Default for Junk {
    fn default() -> Self {
        Self {
            cats: vec![
                Category::new("User Caches", "~/Library/Caches — apps rebuild these", &["Library/Caches"], true),
                Category::new("Logs", "~/Library/Logs and crash reports", &["Library/Logs"], true),
                Category::new(
                    "Xcode Build Data",
                    "DerivedData, iOS device support, simulator caches",
                    &[
                        "Library/Developer/Xcode/DerivedData",
                        "Library/Developer/Xcode/iOS DeviceSupport",
                        "Library/Developer/CoreSimulator/Caches",
                    ],
                    true,
                ),
                Category::new("Developer Package Caches", "npm and Gradle download caches", &[".npm/_cacache", ".gradle/caches"], true),
                Category::new(
                    "ML Model Caches",
                    "Hugging Face / PyTorch downloads — re-downloaded when needed",
                    &[".cache/huggingface", ".cache/torch"],
                    false,
                ),
                Category::new(
                    "Mail Attachments",
                    "Attachments saved by Mail (needs Full Disk Access)",
                    &["Library/Containers/com.apple.mail/Data/Library/Mail Downloads"],
                    false,
                ),
                Category::new("Trash", "Items in your Trash (needs Full Disk Access)", &[".Trash"], true),
            ],
            scan: None,
            clean: None,
            scanned: false,
            confirm: false,
            status: String::new(),
        }
    }
}

impl Junk {
    pub fn is_scanning(&self) -> bool {
        self.scan.is_some()
    }

    pub fn total_found(&self) -> u64 {
        self.cats.iter().map(Category::total).sum()
    }

    fn selected_total(&self) -> u64 {
        self.cats.iter().filter(|c| c.selected).map(Category::total).sum()
    }

    pub fn start_scan(&mut self, ctx: &egui::Context) {
        if self.scan.is_some() {
            return;
        }
        let roots: Vec<_> = self.cats.iter().map(|c| c.roots).collect();
        self.scan = Some(Job::spawn(ctx, move |_, _| {
            let home = fsutil::home();
            roots
                .iter()
                .map(|rs| {
                    let mut items: Items = rs
                        .iter()
                        .flat_map(|r| fsutil::children(&home.join(r)))
                        .filter(|p| p.file_name().is_some_and(|n| n != ".DS_Store"))
                        .collect::<Vec<_>>()
                        .into_par_iter()
                        .map(|p| {
                            let s = fsutil::size_of(&p);
                            (p, s)
                        })
                        .filter(|(_, s)| *s > 0)
                        .collect();
                    items.sort_unstable_by(|a, b| b.1.cmp(&a.1));
                    items
                })
                .collect()
        }));
    }

    /// Call every frame (also from the dashboard) to collect finished work.
    pub fn poll(&mut self, ctx: &egui::Context) {
        if let Some(res) = job::poll(&mut self.scan, ctx) {
            for (c, items) in self.cats.iter_mut().zip(res) {
                c.items = items;
                c.scanned = true;
            }
            self.scanned = true;
        }
        if let Some((freed, failed)) = job::poll(&mut self.clean, ctx) {
            self.status = format!("Freed {}", fmt_size(freed));
            if failed > 0 {
                self.status += &format!(" · {failed} items were in use or protected");
            }
            self.start_scan(ctx);
        }
    }

    pub fn ui(&mut self, ui: &mut Ui) {
        let ctx = ui.ctx().clone();
        self.poll(&ctx);
        let busy = self.scan.is_some() || self.clean.is_some();

        ui.horizontal(|ui| {
            ui.heading("System Junk");
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui.add_enabled(!busy, egui::Button::new("Scan")).clicked() {
                    self.start_scan(&ctx);
                }
                if busy {
                    ui.spinner();
                }
            });
        });
        ui.label(RichText::new("Tip: quit browsers and other apps before cleaning their caches.").weak());
        ui.separator();

        egui::Panel::bottom("junk_bottom").show(ui, |ui| {
            ui.add_space(6.0);
            ui.horizontal(|ui| {
                ui.label(&self.status);
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    let sel = self.selected_total();
                    if self.confirm {
                        if ui.button("Cancel").clicked() {
                            self.confirm = false;
                        }
                        if ui.button(RichText::new("Delete permanently").color(egui::Color32::RED)).clicked() {
                            self.confirm = false;
                            let targets: Items = self
                                .cats
                                .iter()
                                .filter(|c| c.selected)
                                .flat_map(|c| c.items.iter().cloned())
                                .collect();
                            self.clean = Some(Job::spawn(&ctx, move |_, _| fsutil::delete_permanently(&targets)));
                        }
                        ui.label(format!("Delete {} for good?", fmt_size(sel)));
                    } else {
                        let ok = self.scanned && !busy && sel > 0;
                        if ui.add_enabled(ok, egui::Button::new("Clean")).clicked() {
                            self.confirm = true;
                        }
                        ui.label(format!("Selected: {}", fmt_size(sel)));
                    }
                });
            });
            ui.add_space(4.0);
        });

        egui::ScrollArea::vertical().auto_shrink(false).show(ui, |ui| {
            for c in &mut self.cats {
                ui.horizontal(|ui| {
                    ui.checkbox(&mut c.selected, "");
                    let arrow = if c.expanded { "Hide" } else { "Show" };
                    if ui.small_button(arrow).clicked() {
                        c.expanded = !c.expanded;
                    }
                    ui.vertical(|ui| {
                        ui.label(RichText::new(c.name).strong());
                        ui.label(RichText::new(c.detail).weak().small());
                    });
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        ui.label(if c.scanned { fmt_size(c.total()) } else { "—".into() });
                    });
                });
                if c.expanded {
                    ui.indent(c.name, |ui| {
                        if c.items.is_empty() {
                            ui.label(RichText::new("Nothing here").weak());
                        }
                        for (p, s) in c.items.iter().take(200) {
                            ui.horizontal(|ui| {
                                let name = p.file_name().map(|n| n.to_string_lossy()).unwrap_or_default();
                                if ui.link(name).on_hover_text("Reveal in Finder").clicked() {
                                    fsutil::reveal(p);
                                }
                                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                    ui.label(RichText::new(fmt_size(*s)).weak());
                                });
                            });
                        }
                        if c.items.len() > 200 {
                            ui.label(RichText::new(format!("…and {} more", c.items.len() - 200)).weak());
                        }
                    });
                }
                ui.separator();
            }
        });
    }
}
