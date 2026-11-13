mod agents;
mod fsutil;
mod job;
mod junk;
mod large;
mod theme;
mod uninstall;

use eframe::egui::{self, Align2, Color32, Layout, Sense, Ui, vec2};
use job::Job;
use theme::{Palette, icon};

#[derive(PartialEq, Clone, Copy)]
enum Pane {
    Smart,
    Junk,
    Large,
    Apps,
    Agents,
}

impl Pane {
    const ALL: [Pane; 5] = [Pane::Smart, Pane::Junk, Pane::Large, Pane::Apps, Pane::Agents];
    fn title(self) -> &'static str {
        match self {
            Pane::Smart => "Smart Scan",
            Pane::Junk => "System Junk",
            Pane::Large => "Large & Old Files",
            Pane::Apps => "Uninstaller",
            Pane::Agents => "Launch Agents",
        }
    }
    fn glyph(self) -> &'static str {
        match self {
            Pane::Smart => icon::SPARKLE,
            Pane::Junk => icon::BROOM,
            Pane::Large => icon::FOLDER,
            Pane::Apps => icon::PACKAGE,
            Pane::Agents => icon::ROCKET_LAUNCH,
        }
    }
    fn palette(self) -> Palette {
        match self {
            Pane::Smart => theme::SMART,
            Pane::Junk => theme::JUNK,
            Pane::Large => theme::LARGE,
            Pane::Apps => theme::APPS,
            Pane::Agents => theme::AGENTS,
        }
    }
}

/// Live system numbers shown in the sidebar and dashboard.
struct Sys {
    disk: Option<(u64, u64)>,
    trash: Option<u64>,
    fda: bool,
    last_refresh: f64,
    seen_changes: u64,
    trash_job: Option<Job<u64>>,
    empty_job: Option<Job<(u64, usize)>>,
    confirm_empty_until: f64,
}

impl Sys {
    fn new() -> Self {
        Self {
            disk: fsutil::disk_info(),
            trash: None,
            fda: fsutil::has_full_disk_access(),
            last_refresh: 0.0,
            seen_changes: u64::MAX,
            trash_job: None,
            empty_job: None,
            confirm_empty_until: 0.0,
        }
    }

    /// Cheap numbers every 2 s; Trash size whenever something was deleted.
    fn tick(&mut self, ctx: &egui::Context) {
        let now = ctx.input(|i| i.time);
        if now - self.last_refresh > 2.0 {
            self.last_refresh = now;
            self.disk = fsutil::disk_info();
            self.fda = fsutil::has_full_disk_access();
        }
        ctx.request_repaint_after(std::time::Duration::from_secs(2));

        let changes = fsutil::changes();
        if changes != self.seen_changes && self.trash_job.is_none() {
            self.seen_changes = changes;
            self.disk = fsutil::disk_info();
            self.trash_job = Some(Job::spawn(ctx, |_, _| fsutil::trash_size()));
        }
        if let Some(t) = job::poll(&mut self.trash_job, ctx) {
            self.trash = Some(t);
        }
        if job::poll(&mut self.empty_job, ctx).is_some() {
            self.disk = fsutil::disk_info();
        }
    }
}

struct App {
    pane: Pane,
    from: Palette,
    switched_at: f64,
    junk: junk::Junk,
    large: large::Large,
    apps: uninstall::Uninstaller,
    agents: agents::Agents,
    sys: Sys,
}

impl App {
    fn new(cc: &eframe::CreationContext<'_>) -> Self {
        theme::setup(&cc.egui_ctx);
        Self {
            pane: Pane::Smart,
            from: theme::SMART,
            switched_at: -10.0,
            junk: Default::default(),
            large: Default::default(),
            apps: Default::default(),
            agents: Default::default(),
            sys: Sys::new(),
        }
    }

    fn go(&mut self, pane: Pane, now: f64) {
        if pane != self.pane {
            self.from = self.current_palette(now);
            self.pane = pane;
            self.switched_at = now;
        }
    }

    /// Smoothly blends the background between sections.
    fn current_palette(&self, now: f64) -> Palette {
        let t = ((now - self.switched_at) / 0.45).clamp(0.0, 1.0) as f32;
        let ease = 1.0 - (1.0 - t).powi(3);
        theme::lerp_palette(self.from, self.pane.palette(), ease)
    }

    fn sidebar(&mut self, ui: &mut Ui, now: f64) {
        let frame = egui::Frame::new()
            .fill(Color32::from_black_alpha(70))
            .inner_margin(egui::Margin {
                left: 12,
                right: 12,
                top: 44,
                bottom: 14,
            });
        egui::Panel::left("nav")
            .resizable(false)
            .exact_size(230.0)
            .frame(frame)
            .show_separator_line(false)
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.add_space(6.0);
                    ui.label(theme::bold("MacSweep", 20.0));
                });
                ui.add_space(14.0);
                for pane in Pane::ALL {
                    let selected = self.pane == pane;
                    let (rect, resp) = ui.allocate_exact_size(vec2(ui.available_width(), 42.0), Sense::click());
                    let painter = ui.painter();
                    if selected || resp.hovered() {
                        painter.rect_filled(rect, 11, Color32::from_white_alpha(if selected { 45 } else { 18 }));
                    }
                    let tile = egui::Rect::from_center_size(egui::pos2(rect.left() + 24.0, rect.center().y), vec2(28.0, 28.0));
                    painter.rect_filled(
                        tile,
                        8,
                        pane.palette().accent.gamma_multiply(if selected { 1.0 } else { 0.75 }),
                    );
                    painter.text(
                        tile.center(),
                        Align2::CENTER_CENTER,
                        pane.glyph(),
                        egui::FontId::proportional(16.0),
                        Color32::WHITE,
                    );
                    painter.text(
                        egui::pos2(tile.right() + 12.0, rect.center().y),
                        Align2::LEFT_CENTER,
                        pane.title(),
                        theme::semi_font(14.0),
                        Color32::WHITE.gamma_multiply(if selected { 1.0 } else { 0.85 }),
                    );
                    if resp.clicked() {
                        self.go(pane, now);
                    }
                    if resp.hovered() {
                        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
                    }
                    ui.add_space(2.0);
                }

                if !self.sys.fda {
                    ui.add_space(10.0);
                    egui::Frame::new()
                        .fill(theme::WARN.gamma_multiply(0.3))
                        .corner_radius(10)
                        .inner_margin(8)
                        .show(ui, |ui| {
                            ui.set_width(ui.available_width());
                            ui.vertical(|ui| {
                            ui.label(theme::semi(format!("{}  Full Disk Access is off", icon::LOCK_SIMPLE), 12.5));
                            if fsutil::app_bundle().is_none() {
                                ui.label(theme::dim("You're running from Terminal. Open MacSweep.app from Applications instead.", 11.0));
                            } else {
                                ui.label(theme::dim("Turn it on, then relaunch. After a rebuild, remove MacSweep from the list and add it again.", 11.0));
                            }
                            ui.horizontal(|ui| {
                                if ui.link(theme::text("Open Settings", 12.0)).clicked() {
                                    fsutil::open_fda_settings();
                                }
                                if fsutil::app_bundle().is_some() && ui.link(theme::text("Relaunch", 12.0)).clicked() {
                                    fsutil::relaunch();
                                }
                            });
                            });
                        });
                }

                // Live disk + Trash at the bottom — always up to date.
                ui.with_layout(Layout::bottom_up(egui::Align::Min), |ui| {
                    self.trash_row(ui, now);
                    ui.add_space(8.0);
                    if let Some((total, free)) = self.sys.disk {
                        let used = total.saturating_sub(free);
                        let frac = used as f32 / total.max(1) as f32;
                        let color = if frac > 0.9 { theme::DANGER } else { Color32::WHITE };
                        ui.label(theme::dim(
                            format!("{} free of {}", fsutil::fmt_size(free), fsutil::fmt_size(total)),
                            11.5,
                        ));
                        theme::usage_bar(ui, frac, color, 6.0);
                        ui.horizontal(|ui| {
                            ui.label(theme::ic(icon::HARD_DRIVE, 15.0));
                            ui.label(theme::semi("Macintosh HD", 13.0));
                        });
                    }
                });
            });
    }

    fn trash_row(&mut self, ui: &mut Ui, now: f64) {
        let ctx = ui.ctx().clone();
        egui::Frame::new()
            .fill(Color32::from_white_alpha(14))
            .corner_radius(10)
            .inner_margin(8)
            .show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.horizontal(|ui| {
                    ui.label(theme::ic(icon::TRASH, 16.0));
                    ui.vertical(|ui| {
                        ui.label(theme::semi("Trash", 13.0));
                        let size = match (self.sys.trash, self.sys.fda) {
                            (_, false) => "needs access".to_string(),
                            (Some(s), _) => fsutil::fmt_size(s),
                            (None, _) => "…".to_string(),
                        };
                        ui.label(theme::dim(size, 11.5));
                    });
                    ui.with_layout(Layout::right_to_left(egui::Align::Center), |ui| {
                        if self.sys.empty_job.is_some() {
                            ui.spinner();
                            return;
                        }
                        let has = self.sys.trash.unwrap_or(0) > 0;
                        let confirming = now < self.sys.confirm_empty_until;
                        let (label, fill) = if confirming {
                            ("Sure?", theme::DANGER)
                        } else {
                            ("Empty", Color32::from_white_alpha(40))
                        };
                        if ui
                            .add_enabled(has, egui::Button::new(theme::semi(label, 12.0)).fill(fill).corner_radius(12))
                            .clicked()
                        {
                            if confirming {
                                self.sys.confirm_empty_until = 0.0;
                                self.sys.empty_job = Some(Job::spawn(&ctx, |_, _| fsutil::empty_trash()));
                            } else {
                                self.sys.confirm_empty_until = now + 3.0;
                                ctx.request_repaint_after(std::time::Duration::from_secs(3));
                            }
                        }
                    });
                });
            });
    }

    fn smart(&mut self, ui: &mut Ui, p: Palette, now: f64) {
        let ctx = ui.ctx().clone();
        self.junk.poll(&ctx);
        if self.junk.is_busy() {
            let title = if self.junk.is_scanning() {
                "Scanning your Mac…"
            } else {
                "Cleaning up…"
            };
            junk::busy_screen(
                ui,
                p,
                icon::SPARKLE,
                title,
                "Looking through caches, logs, build data and the Trash.",
            );
            return;
        }
        match self.junk.stage {
            junk::Stage::Done(freed, _) => {
                let free = self.sys.disk.map(|d| fsutil::fmt_size(d.1)).unwrap_or_default();
                if junk::done_screen(
                    ui,
                    p,
                    &format!("{} cleaned", fsutil::fmt_size(freed)),
                    &format!("You now have {free} available."),
                ) {
                    self.junk.stage = junk::Stage::Results;
                    self.junk.start_scan(&ctx);
                }
            }
            junk::Stage::Results => {
                theme::centered(ui, |ui| {
                    ui.add_space(ui.available_height() * 0.04);
                    theme::hero(ui, icon::SPARKLE, p, 120.0, false, None);
                    ui.label(theme::bold(fsutil::fmt_size(self.junk.total_found()), 40.0));
                    ui.label(theme::dim("of junk can be cleaned", 15.0));
                    ui.add_space(14.0);
                });
                // Category summary tiles
                let cats: Vec<_> = self
                    .junk
                    .cats
                    .iter()
                    .filter(|c| c.total() > 0)
                    .map(|c| (c.glyph, c.color, c.name, c.total()))
                    .collect();
                let w = 210.0;
                let per_row = ((ui.available_width() + 10.0) / (w + 10.0)).floor().max(1.0) as usize;
                for chunk in cats.chunks(per_row) {
                    ui.with_layout(Layout::left_to_right(egui::Align::Min), |ui| {
                        let used = chunk.len() as f32 * (w + 10.0) - 10.0;
                        ui.add_space(((ui.available_width() - used) / 2.0).max(0.0));
                        for (glyph, color, name, size) in chunk {
                            theme::card(ui, |ui| {
                                ui.set_width(w - 32.0);
                                ui.horizontal(|ui| {
                                    theme::icon_tile(ui, glyph, *color, 34.0);
                                    ui.vertical(|ui| {
                                        ui.label(theme::semi(*name, 13.0));
                                        ui.label(theme::dim(fsutil::fmt_size(*size), 12.5));
                                    });
                                });
                            });
                        }
                    });
                    ui.add_space(4.0);
                }
                theme::centered(ui, |ui| {
                    ui.add_space(12.0);
                    let sel = self.junk.selected_total();
                    if theme::round_button(ui, "Clean", p, sel > 0).clicked() {
                        self.junk.start_clean(&ctx);
                    }
                    ui.label(theme::dim(format!("{} selected", fsutil::fmt_size(sel)), 12.5));
                    if ui.link(theme::text("Review details", 13.5)).clicked() {
                        self.go(Pane::Junk, now);
                    }
                });
            }
            junk::Stage::Intro => {
                theme::centered(ui, |ui| {
                    ui.add_space(ui.available_height() * 0.08);
                    theme::hero(ui, icon::SPARKLE, p, 160.0, false, None);
                    ui.add_space(8.0);
                    ui.label(theme::bold("Welcome to MacSweep", 32.0));
                    ui.label(theme::dim("Run a Smart Scan to find junk you can safely remove.", 15.0));
                    ui.add_space(18.0);
                });
                // Quick stats
                ui.with_layout(Layout::left_to_right(egui::Align::Min), |ui| {
                    let w = 220.0;
                    let n = 3.0;
                    ui.add_space(((ui.available_width() - (w * n + 10.0 * (n - 1.0))) / 2.0).max(0.0));
                    let (free, frac) = self
                        .sys
                        .disk
                        .map(|(t, f)| (fsutil::fmt_size(f), (t - f) as f32 / t.max(1) as f32))
                        .unwrap_or(("—".into(), 0.0));
                    stat(ui, w, icon::HARD_DRIVE, theme::APPS.accent, &free, "available", Some(frac));
                    let trash = self.sys.trash.map(fsutil::fmt_size).unwrap_or_else(|| "—".into());
                    stat(ui, w, icon::TRASH, theme::DANGER, &trash, "in the Trash", None);
                    let access = if self.sys.fda { "On" } else { "Off" };
                    stat(
                        ui,
                        w,
                        icon::LOCK_SIMPLE,
                        if self.sys.fda { theme::JUNK.accent } else { theme::WARN },
                        access,
                        "Full Disk Access",
                        None,
                    );
                });
                theme::centered(ui, |ui| {
                    ui.add_space(22.0);
                    if theme::round_button(ui, "Scan", p, true).clicked() {
                        self.junk.start_scan(&ctx);
                    }
                });
            }
        }
    }
}

fn stat(ui: &mut Ui, w: f32, glyph: &str, color: Color32, value: &str, label: &str, frac: Option<f32>) {
    theme::card(ui, |ui| {
        ui.set_width(w - 32.0);
        ui.horizontal(|ui| {
            theme::icon_tile(ui, glyph, color, 38.0);
            ui.vertical(|ui| {
                ui.label(theme::bold(value, 18.0));
                ui.label(theme::dim(label, 12.0));
            });
        });
        if let Some(f) = frac {
            ui.add_space(4.0);
            theme::usage_bar(ui, f, Color32::WHITE, 5.0);
        }
    });
}

impl eframe::App for App {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        let now = ctx.input(|i| i.time);
        self.sys.tick(&ctx);

        let p = self.current_palette(now);
        if now - self.switched_at < 0.5 {
            ctx.request_repaint();
        }
        let screen = ctx.content_rect();
        theme::paint_background(&ctx.layer_painter(egui::LayerId::background()), screen, p);

        self.sidebar(ui, now);
        egui::CentralPanel::default()
            .frame(egui::Frame::new().inner_margin(26))
            .show(ui, |ui| match self.pane {
                Pane::Smart => self.smart(ui, p, now),
                Pane::Junk => self.junk.ui(ui, p),
                Pane::Large => self.large.ui(ui, p),
                Pane::Apps => self.apps.ui(ui, p),
                Pane::Agents => self.agents.ui(ui, p),
            });
    }
}

fn main() -> eframe::Result {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("MacSweep")
            .with_inner_size([1180.0, 760.0])
            .with_min_inner_size([980.0, 640.0])
            .with_fullsize_content_view(true)
            .with_titlebar_shown(false)
            .with_title_shown(false),
        ..Default::default()
    };
    eframe::run_native("MacSweep", options, Box::new(|cc| Ok(Box::new(App::new(cc)))))
}
