mod agents;
mod dupes;
mod fsutil;
mod job;
mod junk;
mod large;
mod monitor;
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
    Dupes,
    Apps,
    Agents,
    Monitor,
}

impl Pane {
    const ALL: [Pane; 7] = [
        Pane::Smart,
        Pane::Junk,
        Pane::Large,
        Pane::Dupes,
        Pane::Apps,
        Pane::Agents,
        Pane::Monitor,
    ];
    fn title(self) -> &'static str {
        match self {
            Pane::Smart => "Smart Scan",
            Pane::Junk => "System Junk",
            Pane::Large => "Large & Old Files",
            Pane::Dupes => "Duplicates",
            Pane::Apps => "Uninstaller",
            Pane::Agents => "Launch Agents",
            Pane::Monitor => "Menu Bar Monitor",
        }
    }
    fn glyph(self) -> &'static str {
        match self {
            Pane::Smart => icon::SPARKLE,
            Pane::Junk => icon::BROOM,
            Pane::Large => icon::FOLDER,
            Pane::Dupes => icon::COPY,
            Pane::Apps => icon::PACKAGE,
            Pane::Agents => icon::ROCKET_LAUNCH,
            Pane::Monitor => icon::GAUGE,
        }
    }
    fn palette(self) -> Palette {
        match self {
            Pane::Smart => theme::SMART,
            Pane::Junk => theme::JUNK,
            Pane::Large => theme::LARGE,
            Pane::Dupes => theme::DUPES,
            Pane::Apps => theme::APPS,
            Pane::Agents => theme::AGENTS,
            Pane::Monitor => theme::MONITOR,
        }
    }
}

/// Live system numbers shown in the sidebar and dashboard.
struct Sys {
    disk: Option<(u64, u64)>,
    /// Finder's "available" (includes purgeable space), refreshed in the background.
    finder_free: Option<u64>,
    finder_job: Option<Job<Option<u64>>>,
    finder_at: f64,
    disk_name: String,
    trash: Option<u64>,
    fda: bool,
    last_refresh: f64,
    seen_changes: u64,
    trash_sig: u64,
    trash_job: Option<Job<u64>>,
    empty_job: Option<Job<(u64, usize)>>,
    confirm_empty_until: f64,
    /// Result of the last Empty Trash, shown under the Trash row for a while.
    empty_note: Option<(String, f64)>,
}

impl Sys {
    fn new() -> Self {
        Self {
            disk: fsutil::disk_info(),
            finder_free: None,
            finder_job: None,
            finder_at: -100.0,
            disk_name: fsutil::startup_disk_name(),
            trash: None,
            fda: fsutil::has_full_disk_access(),
            last_refresh: 0.0,
            seen_changes: u64::MAX,
            trash_sig: fsutil::trash_signature(),
            trash_job: None,
            empty_job: None,
            confirm_empty_until: 0.0,
            empty_note: None,
        }
    }

    /// Cheap numbers every 2 s; Trash size whenever something was deleted.
    fn tick(&mut self, ctx: &egui::Context) {
        let now = ctx.input(|i| i.time);
        if now - self.last_refresh > 2.0 {
            self.last_refresh = now;
            self.disk = fsutil::disk_info();
            self.fda = fsutil::has_full_disk_access();
            // Things trashed or put back in Finder count too.
            let sig = fsutil::trash_signature();
            if sig != self.trash_sig {
                self.trash_sig = sig;
                self.seen_changes = u64::MAX;
            }
        }
        ctx.request_repaint_after(std::time::Duration::from_secs(2));

        let changes = fsutil::changes();
        if changes != self.seen_changes && self.trash_job.is_none() {
            self.seen_changes = changes;
            self.disk = fsutil::disk_info();
            self.finder_at = -100.0;
            self.trash_job = Some(Job::spawn(ctx, |_, _| fsutil::trash_size()));
        }
        if let Some(t) = job::poll(&mut self.trash_job, ctx) {
            self.trash = Some(t);
        }
        if self.finder_job.is_none() && now - self.finder_at > 15.0 {
            self.finder_at = now;
            self.finder_job = Some(Job::spawn(ctx, |_, _| fsutil::finder_available()));
        }
        if let Some(v) = job::poll(&mut self.finder_job, ctx) {
            self.finder_free = v;
        }
        if let Some((freed, failed)) = job::poll(&mut self.empty_job, ctx) {
            self.disk = fsutil::disk_info();
            let msg = if failed > 0 {
                format!(
                    "{} couldn't be deleted. {} may be in use; try again after quitting apps.",
                    theme::plural(failed, "item"),
                    if failed == 1 { "It" } else { "They" }
                )
            } else {
                format!("Emptied · {} freed", fsutil::fmt_size(freed))
            };
            self.empty_note = Some((msg, now + if failed > 0 { 12.0 } else { 5.0 }));
        }
    }
}

impl Sys {
    /// (total, available) with Finder's notion of available when we have it.
    fn space(&self) -> Option<(u64, u64)> {
        let (total, free) = self.disk?;
        let free = match self.finder_free {
            Some(f) if f >= free && f <= total => f,
            _ => free,
        };
        Some((total, free))
    }
}

struct App {
    pane: Pane,
    /// A background task that crashed, shown as a banner until dismissed.
    failure: Option<String>,
    from: Palette,
    switched_at: f64,
    junk: junk::Junk,
    large: large::Large,
    dupes: dupes::Dupes,
    apps: uninstall::Uninstaller,
    agents: agents::Agents,
    monitor: monitor::Monitor,
    sys: Sys,
}

impl App {
    fn new(cc: &eframe::CreationContext<'_>) -> Self {
        theme::setup(&cc.egui_ctx);
        // If MacSweep was moved or updated, keep the monitor pointing at this copy.
        std::thread::spawn(monitor::repair_on_launch);
        Self {
            failure: None,
            pane: Pane::Smart,
            from: theme::SMART,
            switched_at: -10.0,
            junk: Default::default(),
            large: Default::default(),
            dupes: Default::default(),
            apps: Default::default(),
            agents: Default::default(),
            monitor: Default::default(),
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
                    if let Some((total, free)) = self.sys.space() {
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
                            ui.label(theme::semi(&self.sys.disk_name, 13.0));
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
                if let Some((msg, until)) = &self.sys.empty_note {
                    if now < *until {
                        ui.label(theme::dim(msg, 11.0));
                        ctx.request_repaint_after(std::time::Duration::from_secs_f64(until - now));
                    }
                }
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
                let free = self.sys.space().map(|d| fsutil::fmt_size(d.1)).unwrap_or_default();
                if junk::done_screen(
                    ui,
                    p,
                    &format!("{} cleaned", fsutil::fmt_size(freed)),
                    &format!("You now have {free} available."),
                ) {
                    self.junk.stage = junk::Stage::Results;
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
                    self.junk.clean_bar(ui, p, &ctx);
                    ui.add_space(4.0);
                    if theme::mini_button(ui, "Review details", "See every item and choose what to clean").clicked() {
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
                        .space()
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

        // The title bar is hidden, so the empty strip along the top moves the
        // window like a title bar would (double-click zooms it).
        let strip = egui::Rect::from_min_size(screen.min, vec2(screen.width(), 26.0));
        let drag = ui.interact(strip, egui::Id::unique("window_drag"), Sense::click_and_drag());
        if drag.drag_started() {
            ctx.send_viewport_cmd(egui::ViewportCommand::StartDrag);
        }
        if drag.double_clicked() {
            let max = ctx.input(|i| i.viewport().maximized.unwrap_or(false));
            ctx.send_viewport_cmd(egui::ViewportCommand::Maximized(!max));
        }

        self.sidebar(ui, now);
        if let Some(f) = job::take_failure() {
            self.failure = Some(f);
        }
        if let Some(msg) = self.failure.clone() {
            egui::Panel::top("failure")
                .frame(
                    egui::Frame::new()
                        .fill(theme::DANGER.gamma_multiply(0.85))
                        .inner_margin(egui::Margin::symmetric(16, 8)),
                )
                .show(ui, |ui| {
                    ui.horizontal(|ui| {
                        ui.label(theme::semi(
                            format!(
                                "{}  Something went wrong and that task stopped. Nothing else was changed. ({msg})",
                                icon::WARNING
                            ),
                            12.5,
                        ));
                        ui.with_layout(Layout::right_to_left(egui::Align::Center), |ui| {
                            if ui.link(theme::text("Dismiss", 12.5)).clicked() {
                                self.failure = None;
                            }
                            if ui.link(theme::text("Report", 12.5)).clicked() {
                                fsutil::open("https://github.com/nihal4/macsweep/issues/new");
                            }
                        });
                    });
                });
        }
        egui::CentralPanel::default()
            .frame(egui::Frame::new().inner_margin(26))
            .show(ui, |ui| match self.pane {
                Pane::Smart => self.smart(ui, p, now),
                Pane::Junk => self.junk.ui(ui, p),
                Pane::Large => self.large.ui(ui, p),
                Pane::Dupes => self.dupes.ui(ui, p),
                Pane::Apps => self.apps.ui(ui, p),
                Pane::Agents => self.agents.ui(ui, p),
                Pane::Monitor => self.monitor.ui(ui, p),
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
