//! System Junk: caches, logs, build data, Trash… (also powers Smart Scan).
use crate::fsutil::{self, fmt_size};
use crate::job::{self, Job};
use crate::theme::{self, Palette, icon};
use eframe::egui::{self, Color32, Layout, Ui};
use rayon::prelude::*;
use std::path::PathBuf;

pub struct Item {
    pub path: PathBuf,
    pub size: u64,
    pub selected: bool,
}

pub struct Category {
    pub name: &'static str,
    detail: &'static str,
    pub glyph: &'static str,
    pub color: Color32,
    roots: &'static [&'static str], // relative to ~
    default_on: bool,
    pub items: Vec<Item>,
}

impl Category {
    const fn new(
        name: &'static str,
        detail: &'static str,
        glyph: &'static str,
        color: Color32,
        roots: &'static [&'static str],
        default_on: bool,
    ) -> Self {
        Self {
            name,
            detail,
            glyph,
            color,
            roots,
            default_on,
            items: Vec::new(),
        }
    }
    pub fn total(&self) -> u64 {
        self.items.iter().map(|i| i.size).sum()
    }
    pub fn selected_total(&self) -> u64 {
        self.items.iter().filter(|i| i.selected).map(|i| i.size).sum()
    }
    /// Some(true/false) when all/none are selected, None when mixed.
    fn all_selected(&self) -> Option<bool> {
        let n = self.items.iter().filter(|i| i.selected).count();
        if n == 0 {
            Some(false)
        } else if n == self.items.len() {
            Some(true)
        } else {
            None
        }
    }
}

#[derive(PartialEq, Clone, Copy)]
pub enum Stage {
    Intro,
    Results,
    Done(u64, usize),
}

pub struct Junk {
    pub cats: Vec<Category>,
    scan: Option<Job<Vec<Vec<(PathBuf, u64)>>>>,
    clean: Option<Job<(u64, usize)>>,
    pub stage: Stage,
    focus: usize,
}

impl Default for Junk {
    fn default() -> Self {
        let c = Color32::from_rgb;
        Self {
            cats: vec![
                Category::new(
                    "User Cache Files",
                    "Temporary files apps rebuild on their own",
                    icon::BROOM,
                    c(0x2f, 0xb5, 0x7c),
                    &["Library/Caches"],
                    true,
                ),
                Category::new(
                    "Logs",
                    "Activity and crash logs",
                    icon::FILE_TEXT,
                    c(0x3a, 0x8d, 0xde),
                    &["Library/Logs"],
                    true,
                ),
                Category::new(
                    "Xcode Junk",
                    "DerivedData, old device support, simulator caches",
                    icon::CODE,
                    c(0x4f, 0x6b, 0xf0),
                    &[
                        "Library/Developer/Xcode/DerivedData",
                        "Library/Developer/Xcode/iOS DeviceSupport",
                        "Library/Developer/CoreSimulator/Caches",
                    ],
                    true,
                ),
                Category::new(
                    "Developer Caches",
                    "npm and Gradle download caches",
                    icon::TERMINAL,
                    c(0x8a, 0x5c, 0xe6),
                    &[".npm/_cacache", ".gradle/caches"],
                    true,
                ),
                Category::new(
                    "ML Model Caches",
                    "Hugging Face & PyTorch downloads — re-downloaded when needed",
                    icon::BRAIN,
                    c(0xd9, 0x6c, 0x2b),
                    &[".cache/huggingface", ".cache/torch"],
                    false,
                ),
                Category::new(
                    "Mail Attachments",
                    "Attachments Mail saved on this Mac",
                    icon::ENVELOPE,
                    c(0x2b, 0x9c, 0xc4),
                    &["Library/Containers/com.apple.mail/Data/Library/Mail Downloads"],
                    false,
                ),
                Category::new(
                    "Trash",
                    "Everything in your Trash",
                    icon::TRASH,
                    c(0xe0, 0x4f, 0x5f),
                    &[".Trash"],
                    true,
                ),
            ],
            scan: None,
            clean: None,
            stage: Stage::Intro,
            focus: 0,
        }
    }
}

impl Junk {
    pub fn is_busy(&self) -> bool {
        self.scan.is_some() || self.clean.is_some()
    }
    pub fn is_scanning(&self) -> bool {
        self.scan.is_some()
    }
    pub fn total_found(&self) -> u64 {
        self.cats.iter().map(Category::total).sum()
    }
    pub fn selected_total(&self) -> u64 {
        self.cats.iter().map(Category::selected_total).sum()
    }

    pub fn start_scan(&mut self, ctx: &egui::Context) {
        if self.is_busy() {
            return;
        }
        let roots: Vec<_> = self.cats.iter().map(|c| c.roots).collect();
        self.scan = Some(Job::spawn(ctx, move |_, _| {
            let home = fsutil::home();
            roots
                .iter()
                .map(|rs| {
                    let mut items: Vec<(PathBuf, u64)> = rs
                        .iter()
                        .flat_map(|r| fsutil::children(&home.join(r)))
                        .filter(|p| p.file_name().is_some_and(|n| n != ".DS_Store"))
                        .filter(|p| fsutil::deletable(p))
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

    pub fn start_clean(&mut self, ctx: &egui::Context) {
        if self.is_busy() {
            return;
        }
        let targets: Vec<(PathBuf, u64)> = self
            .cats
            .iter()
            .flat_map(|c| c.items.iter().filter(|i| i.selected).map(|i| (i.path.clone(), i.size)))
            .collect();
        self.clean = Some(Job::spawn(ctx, move |_, _| fsutil::delete_permanently(&targets)));
    }

    /// Call every frame (the dashboard does too) to collect finished work.
    pub fn poll(&mut self, ctx: &egui::Context) {
        if let Some(res) = job::poll(&mut self.scan, ctx) {
            for (c, items) in self.cats.iter_mut().zip(res) {
                let on = c.default_on;
                c.items = items
                    .into_iter()
                    .map(|(path, size)| Item {
                        path,
                        size,
                        selected: on,
                    })
                    .collect();
            }
            self.stage = Stage::Results;
        }
        if let Some((freed, failed)) = job::poll(&mut self.clean, ctx) {
            self.stage = Stage::Done(freed, failed);
            for c in &mut self.cats {
                c.items.retain(|i| i.path.exists());
            }
        }
    }

    pub fn ui(&mut self, ui: &mut Ui, p: Palette) {
        let ctx = ui.ctx().clone();
        self.poll(&ctx);

        if self.is_busy() {
            let title = if self.is_scanning() {
                "Looking for junk…"
            } else {
                "Cleaning up…"
            };
            busy_screen(ui, p, icon::BROOM, title, "This only takes a moment.");
            return;
        }

        match self.stage {
            Stage::Intro => {
                theme::centered(ui, |ui| {
                    ui.add_space(ui.available_height() * 0.10);
                    theme::hero(ui, icon::BROOM, p, 150.0, false, None);
                    ui.add_space(10.0);
                    ui.label(theme::bold("System Junk", 30.0));
                    ui.label(theme::dim(
                        "Clean caches, logs, build leftovers and the Trash\nto free up space and keep your Mac tidy.",
                        15.0,
                    ));
                    ui.add_space(22.0);
                    if theme::round_button(ui, "Scan", p, true).clicked() {
                        self.start_scan(&ctx);
                    }
                });
            }
            Stage::Done(freed, failed) => {
                let note = if failed > 0 {
                    format!("{failed} items were in use or protected and were skipped.")
                } else {
                    "Your Mac is a little lighter now.".into()
                };
                if done_screen(ui, p, &format!("{} cleaned", fmt_size(freed)), &note) {
                    self.start_scan(&ctx);
                }
            }
            Stage::Results => self.results(ui, p, &ctx),
        }
    }

    fn results(&mut self, ui: &mut Ui, p: Palette, ctx: &egui::Context) {
        ui.horizontal(|ui| {
            theme::icon_tile(ui, icon::BROOM, p.accent, 46.0);
            ui.vertical(|ui| {
                ui.label(theme::bold(fmt_size(self.total_found()), 28.0));
                ui.label(theme::dim("of junk found on your Mac", 13.5));
                ui.label(
                    theme::dim(
                        format!(
                            "{}  Browser profiles, passwords, email and app data are never touched",
                            icon::LOCK_SIMPLE
                        ),
                        12.0,
                    )
                    .color(Color32::from_rgb(0xb8, 0xf5, 0xd8)),
                );
            });
            ui.with_layout(Layout::right_to_left(egui::Align::Center), |ui| {
                if theme::pill(
                    ui,
                    format!("{}  Scan again", icon::ARROW_CLOCKWISE),
                    Color32::from_white_alpha(35),
                )
                .clicked()
                {
                    self.start_scan(ctx);
                }
            });
        });
        ui.add_space(10.0);

        egui::Panel::bottom("junk_action").frame(egui::Frame::new()).show(ui, |ui| {
            theme::centered(ui, |ui| {
                ui.add_space(4.0);
                let sel = self.selected_total();
                if theme::round_button(ui, "Clean", p, sel > 0).clicked() {
                    self.start_clean(ctx);
                }
                ui.label(theme::dim(format!("{} selected · removed permanently", fmt_size(sel)), 12.5));
            });
        });

        egui::Panel::left("junk_cats")
            .resizable(false)
            .exact_size(330.0)
            .frame(egui::Frame::new())
            .show(ui, |ui| {
                egui::ScrollArea::vertical().auto_shrink(false).show(ui, |ui| {
                    for (i, c) in self.cats.iter_mut().enumerate() {
                        let resp = theme::row(ui, 58.0, self.focus == i, |ui| {
                            let state = c.all_selected();
                            let mut on = state.unwrap_or(true);
                            let cb = egui::Checkbox::new(&mut on, "").indeterminate(state.is_none());
                            if ui.add_enabled(!c.items.is_empty(), cb).changed() {
                                c.items.iter_mut().for_each(|it| it.selected = on);
                            }
                            theme::icon_tile(ui, c.glyph, c.color, 34.0);
                            ui.vertical(|ui| {
                                ui.add_space(9.0);
                                ui.label(theme::semi(c.name, 14.0));
                                let sub = if c.items.is_empty() {
                                    "Nothing to clean".to_string()
                                } else {
                                    format!("{} items", c.items.len())
                                };
                                ui.label(theme::dim(sub, 11.5));
                            });
                            ui.with_layout(Layout::right_to_left(egui::Align::Center), |ui| {
                                ui.label(theme::semi(fmt_size(c.total()), 13.5));
                            });
                        });
                        if resp.clicked() {
                            self.focus = i;
                        }
                        ui.add_space(2.0);
                    }
                });
            });

        let c = &mut self.cats[self.focus];
        ui.add_space(0.0);
        theme::card(ui, |ui| {
            ui.set_min_size(ui.available_size());
            ui.horizontal(|ui| {
                theme::icon_tile(ui, c.glyph, c.color, 42.0);
                ui.vertical(|ui| {
                    ui.label(theme::bold(c.name, 19.0));
                    ui.label(theme::dim(c.detail, 12.5));
                });
            });
            ui.label(theme::dim(
                format!("{} of {} selected", fmt_size(c.selected_total()), fmt_size(c.total())),
                12.5,
            ));
            ui.separator();
            if c.items.is_empty() {
                theme::centered(ui, |ui| {
                    ui.add_space(60.0);
                    ui.label(theme::ic(icon::CHECK_CIRCLE, 44.0));
                    ui.label(theme::dim("All clean here.", 14.0));
                });
                return;
            }
            let row_h = 28.0;
            egui::ScrollArea::vertical()
                .auto_shrink(false)
                .show_rows(ui, row_h, c.items.len(), |ui, range| {
                    for it in &mut c.items[range] {
                        ui.horizontal(|ui| {
                            ui.set_height(row_h);
                            ui.checkbox(&mut it.selected, "");
                            let name = it
                                .path
                                .file_name()
                                .map(|n| n.to_string_lossy().into_owned())
                                .unwrap_or_default();
                            ui.with_layout(Layout::right_to_left(egui::Align::Center), |ui| {
                                ui.add_space(6.0);
                                ui.label(theme::dim(fmt_size(it.size), 13.0));
                                ui.with_layout(Layout::left_to_right(egui::Align::Center), |ui| {
                                    let resp = ui.add(
                                        egui::Label::new(theme::text(name, 13.5))
                                            .truncate()
                                            .sense(egui::Sense::click()),
                                    );
                                    if resp.on_hover_text("Click to reveal in Finder").clicked() {
                                        fsutil::reveal(&it.path);
                                    }
                                });
                            });
                        });
                    }
                });
        });
    }
}

/// Full-screen "working…" view with spinning hero.
pub fn busy_screen(ui: &mut Ui, p: Palette, glyph: &str, title: &str, sub: &str) {
    theme::centered(ui, |ui| {
        ui.add_space(ui.available_height() * 0.16);
        theme::hero(ui, glyph, p, 150.0, true, None);
        ui.add_space(12.0);
        ui.label(theme::bold(title, 26.0));
        ui.label(theme::dim(sub, 14.0));
    });
}

/// Full-screen "finished" view. Returns true when "Done" is clicked.
pub fn done_screen(ui: &mut Ui, p: Palette, title: &str, sub: &str) -> bool {
    let mut clicked = false;
    theme::centered(ui, |ui| {
        ui.add_space(ui.available_height() * 0.12);
        theme::hero(ui, icon::CHECK_CIRCLE, p, 150.0, false, None);
        ui.add_space(12.0);
        ui.label(theme::bold(title, 32.0));
        ui.label(theme::dim(sub, 15.0));
        ui.add_space(22.0);
        clicked = theme::round_button(ui, "Done", p, true).clicked();
    });
    clicked
}

#[cfg(test)]
mod tests {
    #[test]
    fn every_category_stays_inside_the_allowlist() {
        for c in super::Junk::default().cats {
            for r in c.roots {
                assert!(crate::fsutil::JUNK_ROOTS.contains(r), "{r} is not in JUNK_ROOTS");
            }
        }
    }
}
