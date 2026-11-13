//! Launch agents & daemons, with orphan detection.
use crate::fsutil;
use crate::job::{self, Job};
use eframe::egui::{self, Color32, RichText, Ui};
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
    confirm: bool,
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
    Item { path: path.to_owned(), label, program, scope, is_user, orphaned, selected: false }
}

fn remove(targets: Vec<(PathBuf, bool)>) -> String {
    let uid = unsafe { libc::getuid() };
    for (p, is_user) in &targets {
        if *is_user {
            let _ = Command::new("launchctl").args(["bootout", &format!("gui/{uid}")]).arg(p).status();
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
            (PathBuf::from("/Library/LaunchDaemons"), "Daemon", false),
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
        self.items.sort_by(|a, b| (!a.orphaned, &a.label).cmp(&(!b.orphaned, &b.label)));
    }

    pub fn ui(&mut self, ui: &mut Ui) {
        let ctx = ui.ctx().clone();
        if !self.loaded {
            self.loaded = true;
            self.load();
        }
        if let Some(msg) = job::poll(&mut self.remove, &ctx) {
            self.status = msg;
            self.load();
        }

        ui.horizontal(|ui| {
            ui.heading("Launch Agents & Daemons");
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui.button("Refresh").clicked() {
                    self.load();
                }
            });
        });
        ui.label(RichText::new("Background helpers that start with your Mac. \"Orphaned\" means the app behind it is gone.").weak());
        ui.separator();

        egui::Panel::bottom("agents_bottom").show(ui, |ui| {
            ui.add_space(6.0);
            ui.horizontal(|ui| {
                ui.label(&self.status);
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    let n = self.items.iter().filter(|i| i.selected).count();
                    if self.confirm {
                        if ui.button("Cancel").clicked() {
                            self.confirm = false;
                        }
                        if ui.button(RichText::new("Move to Trash").color(Color32::RED)).clicked() {
                            self.confirm = false;
                            let targets: Vec<_> = self.items.iter().filter(|i| i.selected).map(|i| (i.path.clone(), i.is_user)).collect();
                            self.remove = Some(Job::spawn(&ctx, move |_, _| remove(targets)));
                        }
                        ui.label(format!("Remove {n} item(s)? Only remove ones you recognise."));
                    } else if ui.add_enabled(n > 0 && self.remove.is_none(), egui::Button::new("Remove Selected")).clicked() {
                        self.confirm = true;
                    }
                });
            });
            ui.add_space(4.0);
        });

        egui::ScrollArea::vertical().auto_shrink(false).show(ui, |ui| {
            egui::Grid::new("agents").striped(true).num_columns(5).show(ui, |ui| {
                for h in ["", "Label", "Type", "Status", "Program"] {
                    ui.label(RichText::new(h).strong());
                }
                ui.end_row();
                for it in &mut self.items {
                    ui.checkbox(&mut it.selected, "");
                    if ui.link(&it.label).on_hover_text("Reveal plist in Finder").clicked() {
                        fsutil::reveal(&it.path);
                    }
                    ui.label(it.scope);
                    if it.orphaned {
                        ui.label(RichText::new("Orphaned").color(Color32::from_rgb(230, 140, 30)));
                    } else {
                        ui.label(RichText::new("OK").weak());
                    }
                    ui.add(egui::Label::new(RichText::new(&it.program).weak()).truncate());
                    ui.end_row();
                }
            });
        });
    }
}
