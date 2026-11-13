mod agents;
mod fsutil;
mod job;
mod junk;
mod large;
mod uninstall;

use eframe::egui::{self, RichText};

#[derive(PartialEq, Clone, Copy)]
enum Pane {
    Dashboard,
    Junk,
    Large,
    Apps,
    Agents,
}

const PANES: [(Pane, &str); 5] = [
    (Pane::Dashboard, "Dashboard"),
    (Pane::Junk, "System Junk"),
    (Pane::Large, "Large Files"),
    (Pane::Apps, "Uninstaller"),
    (Pane::Agents, "Launch Agents"),
];

struct App {
    pane: Pane,
    junk: junk::Junk,
    large: large::Large,
    apps: uninstall::Uninstaller,
    agents: agents::Agents,
    disk: Option<(u64, u64)>,
    fda: bool,
}

impl App {
    fn new() -> Self {
        Self {
            pane: Pane::Dashboard,
            junk: Default::default(),
            large: Default::default(),
            apps: Default::default(),
            agents: Default::default(),
            disk: fsutil::disk_info(),
            fda: fsutil::has_full_disk_access(),
        }
    }

    fn dashboard(&mut self, ui: &mut egui::Ui) {
        let ctx = ui.ctx().clone();
        let was_scanning = self.junk.is_scanning();
        self.junk.poll(&ctx);
        if was_scanning && !self.junk.is_scanning() {
            self.disk = fsutil::disk_info();
        }

        ui.heading(RichText::new("MacSweep").size(28.0));
        ui.add_space(10.0);

        if !self.fda {
            egui::Frame::group(ui.style()).show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.label(RichText::new("Full Disk Access is off").strong());
                    ui.label(RichText::new("— Trash, Mail and some app data can't be scanned.").weak());
                    if ui.button("Open Settings").clicked() {
                        fsutil::open_fda_settings();
                    }
                    if ui.button("Re-check").clicked() {
                        self.fda = fsutil::has_full_disk_access();
                    }
                });
            });
            ui.add_space(10.0);
        }

        egui::Frame::group(ui.style()).show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.label(RichText::new("Startup Disk").strong());
            match self.disk {
                Some((total, free)) => {
                    let used = total.saturating_sub(free);
                    let frac = used as f32 / total.max(1) as f32;
                    let bar = egui::ProgressBar::new(frac).fill(if frac > 0.9 {
                        egui::Color32::from_rgb(220, 60, 60)
                    } else {
                        ui.visuals().selection.bg_fill
                    });
                    ui.add(bar);
                    ui.label(format!(
                        "{} used · {} available of {}",
                        fsutil::fmt_size(used),
                        fsutil::fmt_size(free),
                        fsutil::fmt_size(total)
                    ));
                }
                None => {
                    ui.label("Couldn't read disk info");
                }
            }
        });
        ui.add_space(10.0);

        egui::Frame::group(ui.style()).show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.label(RichText::new("Smart Scan").strong());
            ui.horizontal(|ui| {
                if self.junk.is_scanning() {
                    ui.spinner();
                    ui.label("Scanning for junk…");
                } else if self.junk.scanned {
                    ui.label(RichText::new(format!("{} of junk found", fsutil::fmt_size(self.junk.total_found()))).size(20.0).strong());
                    if ui.button("Review").clicked() {
                        self.pane = Pane::Junk;
                    }
                } else {
                    ui.label("Find caches, logs, build data and other junk.");
                }
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    let label = if self.junk.scanned { "Scan Again" } else { "Scan" };
                    if ui.add_enabled(!self.junk.is_scanning(), egui::Button::new(label)).clicked() {
                        self.junk.start_scan(&ctx);
                    }
                });
            });
        });
        ui.add_space(10.0);

        ui.horizontal(|ui| {
            for (pane, title, text) in [
                (Pane::Large, "Large Files", "Find big, forgotten files"),
                (Pane::Apps, "Uninstaller", "Remove apps and their leftovers"),
                (Pane::Agents, "Launch Agents", "Background items & orphans"),
            ] {
                let card = egui::Frame::group(ui.style()).show(ui, |ui| {
                    ui.vertical(|ui| {
                        ui.set_width(200.0);
                        ui.label(RichText::new(title).strong().size(16.0));
                        ui.label(RichText::new(text).weak());
                    });
                });
                if card.response.interact(egui::Sense::click()).clicked() {
                    self.pane = pane;
                }
            }
        });
    }
}

impl eframe::App for App {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        egui::Panel::left("nav").resizable(false).exact_size(180.0).show(ui, |ui| {
            ui.add_space(8.0);
            for (pane, label) in PANES {
                if ui.selectable_label(self.pane == pane, RichText::new(label).size(15.0)).clicked() {
                    self.pane = pane;
                }
                ui.add_space(2.0);
            }
        });
        egui::CentralPanel::default().show(ui, |ui| match self.pane {
            Pane::Dashboard => self.dashboard(ui),
            Pane::Junk => self.junk.ui(ui),
            Pane::Large => self.large.ui(ui),
            Pane::Apps => self.apps.ui(ui),
            Pane::Agents => self.agents.ui(ui),
        });
    }
}

fn main() -> eframe::Result {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("MacSweep")
            .with_inner_size([1000.0, 640.0])
            .with_min_inner_size([820.0, 520.0]),
        ..Default::default()
    };
    eframe::run_native("MacSweep", options, Box::new(|_cc| Ok(Box::new(App::new()))))
}
