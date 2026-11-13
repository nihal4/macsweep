//! Launch agents & daemons, with orphan detection.
use crate::fsutil;
use crate::job::{self, Job};
use crate::theme::{self, Palette, icon};
use eframe::egui::{self, Color32, Layout, Ui};
use std::path::{Path, PathBuf};
use std::process::Command;

pub struct Item {
    path: PathBuf,
    label: String,
    program: String,
    scope: &'static str,
    is_user: bool,
    orphaned: bool,
    selected: bool,
}

#[derive(Default)]
pub struct Agents {
    items: Vec<Item>,
    loaded: bool,
    remove: Option<Job<String>>,
    status: String,
}

fn parse(path: &Path, scope: &'static str, is_user: bool) -> Item {
    let v = plist::Value::from_file(path).ok();
    let d = v.as_ref().and_then(|v| v.as_dictionary());
    let label = d
        .and_then(|d| d.get("Label")?.as_string().map(str::to_owned))
        .unwrap_or_else(|| path.file_stem().unwrap_or_default().to_string_lossy().into_owned());
    let program = d
        .and_then(|d| {
            d.get("Program")
                .and_then(|p| p.as_string())
                .or_else(|| d.get("ProgramArguments")?.as_array()?.first()?.as_string())
                .map(str::to_owned)
        })
        .unwrap_or_default();
    let orphaned = program.starts_with('/') && !Path::new(&program).exists();
    Item {
        path: path.to_owned(),
        label,
        program,
        scope,
        is_user,
        orphaned,
        selected: false,
    }
}

fn remove(targets: Vec<(PathBuf, bool)>) -> String {
    let uid = unsafe { libc::getuid() };
    for (p, is_user) in &targets {
        if *is_user {
            let _ = Command::new("launchctl")
                .args(["bootout", &format!("gui/{uid}")])
                .arg(p)
                .status();
        }
    }
    let paths: Vec<PathBuf> = targets.into_iter().map(|t| t.0).collect();
    match fsutil::move_to_trash(&paths) {
        Ok(()) => format!("Removed {} item(s). System items stop after a restart.", paths.len()),
        Err(e) => format!("Couldn't remove everything: {e}"),
    }
}

impl Agents {
    fn load(&mut self) {
        let sources = [
            (fsutil::home().join("Library/LaunchAgents"), "User agent", true),
            (PathBuf::from("/Library/LaunchAgents"), "Global agent", false),
            (PathBuf::from("/Library/LaunchDaemons"), "System daemon", false),
        ];
        self.items = sources
            .iter()
            .flat_map(|(dir, scope, user)| {
                fsutil::children(dir)
                    .into_iter()
                    .filter(|p| p.extension().is_some_and(|e| e == "plist"))
                    .map(move |p| parse(&p, scope, *user))
            })
            .collect();
        self.items
            .sort_by(|a, b| (!a.orphaned, &a.label).cmp(&(!b.orphaned, &b.label)));
    }

    pub fn ui(&mut self, ui: &mut Ui, p: Palette) {
        let ctx = ui.ctx().clone();
        if !self.loaded {
            self.loaded = true;
            self.load();
        }
        if let Some(msg) = job::poll(&mut self.remove, &ctx) {
            self.status = msg;
            self.load();
        }

        let orphans = self.items.iter().filter(|i| i.orphaned).count();
        ui.horizontal(|ui| {
            theme::icon_tile(ui, icon::ROCKET_LAUNCH, p.accent, 46.0);
            ui.vertical(|ui| {
                ui.label(theme::bold(format!("{} background items", self.items.len()), 28.0));
                let sub = if orphans > 0 {
                    format!("{orphans} orphaned — their app is already gone")
                } else {
                    "No orphaned items found".into()
                };
                ui.label(theme::dim(sub, 13.5));
            });
            ui.with_layout(Layout::right_to_left(egui::Align::Center), |ui| {
                if theme::pill(
                    ui,
                    format!("{}  Refresh", icon::ARROW_CLOCKWISE),
                    Color32::from_white_alpha(35),
                )
                .clicked()
                {
                    self.load();
                }
            });
        });
        ui.add_space(10.0);

        egui::Panel::bottom("agents_action").frame(egui::Frame::new()).show(ui, |ui| {
            theme::centered(ui, |ui| {
                ui.add_space(4.0);
                let n = self.items.iter().filter(|i| i.selected).count();
                if theme::round_button(ui, "Remove", p, n > 0 && self.remove.is_none()).clicked() {
                    let targets: Vec<_> = self
                        .items
                        .iter()
                        .filter(|i| i.selected)
                        .map(|i| (i.path.clone(), i.is_user))
                        .collect();
                    self.remove = Some(Job::spawn(&ctx, move |_, _| remove(targets)));
                }
                let msg = if !self.status.is_empty() {
                    self.status.clone()
                } else if n == 0 {
                    "Only remove items you recognise".into()
                } else {
                    format!("{n} selected · moved to Trash")
                };
                ui.label(theme::dim(msg, 12.5));
            });
        });

        theme::card(ui, |ui| {
            ui.set_min_size(ui.available_size());
            if self.items.is_empty() {
                theme::centered(ui, |ui| {
                    ui.add_space(60.0);
                    ui.label(theme::ic(icon::CHECK_CIRCLE, 44.0));
                    ui.label(theme::dim("No launch agents or daemons found.", 14.0));
                });
                return;
            }
            egui::ScrollArea::vertical().auto_shrink(false).show(ui, |ui| {
                for it in &mut self.items {
                    egui::Frame::new()
                        .fill(Color32::from_white_alpha(10))
                        .corner_radius(12)
                        .inner_margin(10)
                        .show(ui, |ui| {
                            ui.set_width(ui.available_width());
                            ui.horizontal(|ui| {
                                ui.checkbox(&mut it.selected, "");
                                let (glyph, color) = if it.orphaned {
                                    (icon::GHOST, theme::WARN)
                                } else {
                                    (icon::GEAR_SIX, p.accent.gamma_multiply(0.8))
                                };
                                theme::icon_tile(ui, glyph, color, 34.0);
                                ui.with_layout(Layout::right_to_left(egui::Align::Center), |ui| {
                                    if it.orphaned {
                                        theme::badge(ui, "Orphaned", theme::WARN);
                                    }
                                    theme::badge(ui, it.scope, Color32::from_white_alpha(120));
                                    ui.with_layout(Layout::top_down(egui::Align::Min), |ui| {
                                        let r = ui.add(
                                            egui::Label::new(theme::semi(&it.label, 14.0))
                                                .truncate()
                                                .sense(egui::Sense::click()),
                                        );
                                        if r.on_hover_text("Reveal plist in Finder").clicked() {
                                            fsutil::reveal(&it.path);
                                        }
                                        ui.add(egui::Label::new(theme::dim(&it.program, 11.5)).truncate());
                                    });
                                });
                            });
                        });
                    ui.add_space(4.0);
                }
            });
        });
    }
}
