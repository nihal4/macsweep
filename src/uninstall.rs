//! App Uninstaller: every app with its real icon, details and leftovers.
use crate::fsutil::{self, fmt_size};
use crate::job::{self, Job};
use crate::junk::{busy_screen, done_screen};
use crate::theme::{self, Palette, icon};
use eframe::egui::{self, Color32, ColorImage, Layout, TextureHandle, TextureOptions, Ui};
use rayon::prelude::*;
use std::collections::HashMap;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::Command;

const ICON_PX: usize = 64;

#[derive(Clone, Copy, PartialEq)]
enum Arch {
    Universal,
    AppleSilicon,
    Intel,
    Unknown,
}

#[derive(Clone)]
struct Leftover {
    path: PathBuf,
    size: u64,
    kind: &'static str,
    selected: bool,
}

#[derive(Clone)]
pub struct AppInfo {
    path: PathBuf,
    name: String,
    version: Option<String>,
    bundle_id: Option<String>,
    size: u64,
    arch: Arch,
    app_store: bool,
    icon_file: Option<String>,
    leftovers: Vec<Leftover>,
    checked: bool,
}

impl AppInfo {
    fn leftovers_selected(&self) -> u64 {
        self.leftovers.iter().filter(|l| l.selected).map(|l| l.size).sum()
    }
    fn total(&self) -> u64 {
        self.size + self.leftovers_selected()
    }
}

#[derive(Default)]
pub struct Uninstaller {
    apps: Vec<AppInfo>,
    loaded: bool,
    query: String,
    sort_size: bool,
    focus: Option<PathBuf>,
    icons: HashMap<PathBuf, TextureHandle>,
    last_opened: HashMap<PathBuf, String>,
    scan: Option<Job<Vec<AppInfo>>>,
    icon_job: Option<Job<Vec<(PathBuf, ColorImage)>>>,
    opened_job: Option<Job<(PathBuf, String)>>,
    remove: Option<Job<(usize, u64, Option<String>)>>,
    done: Option<(usize, u64, Option<String>)>,
}

// ---------- discovery ----------

fn plist_str(d: &plist::Dictionary, key: &str) -> Option<String> {
    d.get(key).and_then(|v| v.as_string()).map(str::to_owned)
}

/// Reads the Mach-O header of the main executable.
fn detect_arch(app: &Path, exe: Option<&str>) -> Arch {
    let Some(exe) = exe else { return Arch::Unknown };
    let Ok(mut f) = std::fs::File::open(app.join("Contents/MacOS").join(exe)) else {
        return Arch::Unknown;
    };
    let mut buf = [0u8; 4096];
    let n = f.read(&mut buf).unwrap_or(0);
    if n < 8 {
        return Arch::Unknown;
    }
    let be = |o: usize| u32::from_be_bytes(buf[o..o + 4].try_into().unwrap());
    let le = |o: usize| u32::from_le_bytes(buf[o..o + 4].try_into().unwrap());
    const ARM64: u32 = 0x0100_000C;
    const X86_64: u32 = 0x0100_0007;
    let (mut arm, mut intel) = (false, false);
    match be(0) {
        0xCAFE_BABE | 0xCAFE_BABF => {
            let entry = if be(0) == 0xCAFE_BABE { 20 } else { 32 };
            let count = be(4).min(16) as usize;
            for i in 0..count {
                let o = 8 + i * entry;
                if o + 4 > n {
                    break;
                }
                match be(o) {
                    ARM64 => arm = true,
                    X86_64 => intel = true,
                    _ => {}
                }
            }
        }
        _ if le(0) == 0xFEED_FACF => match le(4) {
            ARM64 => arm = true,
            X86_64 => intel = true,
            _ => {}
        },
        _ => {}
    }
    match (arm, intel) {
        (true, true) => Arch::Universal,
        (true, false) => Arch::AppleSilicon,
        (false, true) => Arch::Intel,
        _ => Arch::Unknown,
    }
}

const LIB_DIRS: [(&str, &str); 13] = [
    ("Application Support", "App Support"),
    ("Caches", "Cache"),
    ("Preferences", "Preferences"),
    ("Preferences/ByHost", "Preferences"),
    ("Containers", "Container"),
    ("Group Containers", "Group Container"),
    ("Saved Application State", "Saved State"),
    ("Logs", "Logs"),
    ("HTTPStorages", "Web Data"),
    ("WebKit", "Web Data"),
    ("Cookies", "Cookies"),
    ("LaunchAgents", "Launch Agent"),
    ("Application Scripts", "Scripts"),
];

/// One listing of ~/Library, shared by every app's leftover search.
fn library_index() -> Vec<(PathBuf, String, &'static str)> {
    let lib = fsutil::home().join("Library");
    LIB_DIRS
        .iter()
        .flat_map(|(dir, label)| {
            fsutil::children(&lib.join(dir)).into_iter().map(move |p| {
                let n = p.file_name().map(|n| n.to_string_lossy().to_lowercase()).unwrap_or_default();
                (p, n, *label)
            })
        })
        .collect()
}

fn find_apps() -> Vec<AppInfo> {
    let mut bundles = Vec::new();
    for root in [PathBuf::from("/Applications"), fsutil::home().join("Applications")] {
        for c in fsutil::children(&root) {
            if c.extension().is_some_and(|e| e == "app") {
                bundles.push(c);
            } else if c.is_dir() {
                bundles.extend(
                    fsutil::children(&c)
                        .into_iter()
                        .filter(|p| p.extension().is_some_and(|e| e == "app")),
                );
            }
        }
    }
    let index = library_index();
    let mut apps: Vec<AppInfo> = bundles
        .into_par_iter()
        .filter_map(|path| {
            let real = std::fs::canonicalize(&path).unwrap_or_else(|_| path.clone());
            let info = plist::Value::from_file(real.join("Contents/Info.plist")).ok();
            let d = info.as_ref().and_then(|v| v.as_dictionary());
            let bundle_id = d.and_then(|d| plist_str(d, "CFBundleIdentifier"));
            if bundle_id.as_deref().is_some_and(|b| b.starts_with("com.apple.")) {
                return None; // never touch Apple's apps
            }
            let name = path.file_stem()?.to_string_lossy().into_owned();
            let bundle_name = d.and_then(|d| plist_str(d, "CFBundleName"));
            let exe = d.and_then(|d| plist_str(d, "CFBundleExecutable"));

            // Leftovers: match bundle ID or exact app name.
            let mut names = vec![name.to_lowercase(), name.to_lowercase().replace(' ', "")];
            if let Some(b) = &bundle_name {
                names.push(b.to_lowercase());
            }
            let bid = bundle_id.as_ref().map(|b| b.to_lowercase());
            let leftovers: Vec<Leftover> = index
                .iter()
                .filter(|(_, n, _)| {
                    let stem = Path::new(n)
                        .file_stem()
                        .map(|s| s.to_string_lossy().into_owned())
                        .unwrap_or_default();
                    names.iter().any(|x| x == n || *x == stem) || bid.as_ref().is_some_and(|b| n.contains(b.as_str()))
                })
                .map(|(p, _, kind)| Leftover {
                    path: p.clone(),
                    size: fsutil::size_of(p),
                    kind,
                    selected: true,
                })
                .collect();

            Some(AppInfo {
                version: d.and_then(|d| plist_str(d, "CFBundleShortVersionString")),
                icon_file: d.and_then(|d| plist_str(d, "CFBundleIconFile")),
                arch: detect_arch(&real, exe.as_deref()),
                app_store: real.join("Contents/_MASReceipt").exists(),
                size: fsutil::app_size(&path),
                bundle_id,
                name,
                path,
                leftovers,
                checked: false,
            })
        })
        .collect();
    for a in &mut apps {
        a.leftovers.sort_unstable_by(|x, y| y.size.cmp(&x.size));
    }
    apps.sort_by_key(|a| a.name.to_lowercase());
    apps
}

// ---------- icons ----------

/// Decodes the app's .icns into a small 64×64 RGBA image.
fn load_icon(app: &Path, icon_file: Option<&str>) -> Option<ColorImage> {
    let real = std::fs::canonicalize(app).ok()?;
    let mut name = icon_file.unwrap_or("AppIcon").to_string();
    if !name.ends_with(".icns") {
        name.push_str(".icns");
    }
    let file = std::fs::File::open(real.join("Contents/Resources").join(name)).ok()?;
    let family = icns::IconFamily::read(std::io::BufReader::new(file)).ok()?;
    let mut types = family.available_icons();
    // Smallest icon that is still >= 64px first, then bigger ones, then smaller.
    types.sort_by_key(|t| {
        let w = t.pixel_width();
        if w >= ICON_PX as u32 { w } else { 100_000 - w }
    });
    for t in types {
        let Ok(img) = family.get_icon_with_type(t) else { continue };
        let img = img.convert_to(icns::PixelFormat::RGBA);
        return Some(downscale(img.data(), img.width() as usize, img.height() as usize));
    }
    None
}

/// Box-filter resize to ICON_PX with alpha weighting (keeps edges clean).
fn downscale(src: &[u8], w: usize, h: usize) -> ColorImage {
    let mut out = vec![0u8; ICON_PX * ICON_PX * 4];
    for oy in 0..ICON_PX {
        for ox in 0..ICON_PX {
            let (x0, x1) = (ox * w / ICON_PX, ((ox + 1) * w / ICON_PX).max(ox * w / ICON_PX + 1).min(w));
            let (y0, y1) = (oy * h / ICON_PX, ((oy + 1) * h / ICON_PX).max(oy * h / ICON_PX + 1).min(h));
            let (mut r, mut g, mut b, mut a, mut n) = (0u64, 0u64, 0u64, 0u64, 0u64);
            for y in y0..y1 {
                for x in x0..x1 {
                    let i = (y * w + x) * 4;
                    let al = src[i + 3] as u64;
                    r += src[i] as u64 * al;
                    g += src[i + 1] as u64 * al;
                    b += src[i + 2] as u64 * al;
                    a += al;
                    n += 1;
                }
            }
            let o = (oy * ICON_PX + ox) * 4;
            if a > 0 {
                out[o] = (r / a) as u8;
                out[o + 1] = (g / a) as u8;
                out[o + 2] = (b / a) as u8;
                out[o + 3] = (a / n.max(1)) as u8;
            }
        }
    }
    ColorImage::from_rgba_unmultiplied([ICON_PX, ICON_PX], &out)
}

fn last_opened(app: &Path) -> String {
    let out = Command::new("mdls")
        .args(["-raw", "-name", "kMDItemLastUsedDate"])
        .arg(app)
        .output();
    let s = out
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
        .unwrap_or_default();
    if s.is_empty() || s == "(null)" {
        "Never / unknown".into()
    } else {
        s.chars().take(10).collect()
    }
}

fn osascript(script: &str) -> String {
    Command::new("osascript")
        .args(["-e", script])
        .output()
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
        .unwrap_or_default()
}

/// Quits each app if running, then moves app + selected leftovers to the Trash.
fn uninstall(apps: Vec<AppInfo>) -> (usize, u64, Option<String>) {
    let mut paths = Vec::new();
    let mut freed = 0;
    let mut still_running = Vec::new();
    for a in &apps {
        if let Some(bid) = &a.bundle_id {
            let running = format!("application id \"{bid}\" is running");
            if osascript(&running) == "true" {
                osascript(&format!("tell application id \"{bid}\" to quit"));
                std::thread::sleep(std::time::Duration::from_millis(1500));
                if osascript(&running) == "true" {
                    still_running.push(a.name.clone());
                    continue;
                }
            }
        }
        paths.push(a.path.clone());
        paths.extend(a.leftovers.iter().filter(|l| l.selected).map(|l| l.path.clone()));
        freed += a.total();
    }
    let n = apps.len() - still_running.len();
    let mut err = fsutil::move_to_trash(&paths).err();
    if !still_running.is_empty() {
        err = Some(format!(
            "{} is still running — quit it and try again.",
            still_running.join(", ")
        ));
    }
    (n, freed, err)
}

// ---------- UI ----------

fn arch_badge(ui: &mut Ui, arch: Arch) {
    match arch {
        Arch::Universal => theme::badge(ui, "Universal", Color32::from_rgb(0x5a, 0xa8, 0xff)),
        Arch::AppleSilicon => theme::badge(ui, "Apple Silicon", Color32::from_rgb(0x34, 0xd9, 0x9a)),
        Arch::Intel => theme::badge(ui, "Intel · runs via Rosetta", theme::WARN),
        Arch::Unknown => {}
    }
}

fn letter_color(name: &str) -> Color32 {
    let h = name.bytes().fold(7u32, |h, b| h.wrapping_mul(31).wrapping_add(b as u32));
    let palette = [
        (0x5a, 0x8d, 0xee),
        (0xe0, 0x5c, 0x8a),
        (0x34, 0xb5, 0x8a),
        (0xe0, 0x8a, 0x34),
        (0x8a, 0x5c, 0xe6),
        (0x2b, 0x9c, 0xc4),
    ];
    let (r, g, b) = palette[h as usize % palette.len()];
    Color32::from_rgb(r, g, b)
}

impl Uninstaller {
    fn rescan(&mut self, ctx: &egui::Context) {
        self.scan = Some(Job::spawn(ctx, |_, _| find_apps()));
    }

    fn app_icon(&self, ui: &mut Ui, app: &AppInfo, size: f32) {
        if let Some(tex) = self.icons.get(&app.path) {
            ui.add(egui::Image::new(tex).fit_to_exact_size(egui::vec2(size, size)));
        } else {
            let letter = app.name.chars().next().unwrap_or('?').to_uppercase().to_string();
            theme::icon_tile(ui, &letter, letter_color(&app.name), size);
        }
    }

    pub fn ui(&mut self, ui: &mut Ui, p: Palette) {
        let ctx = ui.ctx().clone();
        if !self.loaded {
            self.loaded = true;
            self.rescan(&ctx);
        }
        if let Some(apps) = job::poll(&mut self.scan, &ctx) {
            // keep the user's checkmarks across rescans
            let checked: Vec<PathBuf> = self.apps.iter().filter(|a| a.checked).map(|a| a.path.clone()).collect();
            self.apps = apps;
            for a in &mut self.apps {
                a.checked = checked.contains(&a.path);
            }
            let need: Vec<(PathBuf, Option<String>)> = self
                .apps
                .iter()
                .filter(|a| !self.icons.contains_key(&a.path))
                .map(|a| (a.path.clone(), a.icon_file.clone()))
                .collect();
            if !need.is_empty() {
                self.icon_job = Some(Job::spawn(&ctx, move |_, _| {
                    need.into_par_iter()
                        .filter_map(|(p, f)| load_icon(&p, f.as_deref()).map(|img| (p, img)))
                        .collect()
                }));
            }
            if self.focus.as_ref().is_none_or(|f| !self.apps.iter().any(|a| &a.path == f)) {
                self.focus = None;
            }
        }
        if let Some(icons) = job::poll(&mut self.icon_job, &ctx) {
            for (path, img) in icons {
                let tex = ctx.load_texture(path.to_string_lossy(), img, TextureOptions::LINEAR);
                self.icons.insert(path, tex);
            }
        }
        if let Some((path, s)) = job::poll(&mut self.opened_job, &ctx) {
            self.last_opened.insert(path, s);
        }
        if let Some(res) = job::poll(&mut self.remove, &ctx) {
            self.done = Some(res);
            self.focus = None;
            self.rescan(&ctx);
        }

        if self.remove.is_some() {
            busy_screen(
                ui,
                p,
                icon::PACKAGE,
                "Uninstalling…",
                "Quitting apps and moving them to the Trash.",
            );
            return;
        }
        if let Some((n, freed, err)) = &self.done {
            let sub = err
                .clone()
                .unwrap_or_else(|| "Empty the Trash (bottom-left) to get the space back.".into());
            if done_screen(
                ui,
                p,
                &format!("{n} app{} removed · {}", if *n == 1 { "" } else { "s" }, fmt_size(*freed)),
                &sub,
            ) {
                self.done = None;
            }
            return;
        }
        if self.apps.is_empty() {
            if self.scan.is_some() {
                busy_screen(
                    ui,
                    p,
                    icon::PACKAGE,
                    "Measuring your apps…",
                    "Finding every app and the files it left around.",
                );
            } else {
                theme::centered(ui, |ui| {
                    ui.add_space(120.0);
                    ui.label(theme::dim("No apps found.", 15.0));
                });
            }
            return;
        }

        self.header(ui, &ctx);
        ui.add_space(10.0);
        self.bottom(ui, p, &ctx);
        self.list(ui, &ctx);
        self.detail(ui, &ctx);
    }

    fn header(&mut self, ui: &mut Ui, ctx: &egui::Context) {
        let total: u64 = self
            .apps
            .iter()
            .map(|a| a.size + a.leftovers.iter().map(|l| l.size).sum::<u64>())
            .sum();
        ui.horizontal(|ui| {
            theme::icon_tile(ui, icon::PACKAGE, theme::APPS.accent, 46.0);
            ui.vertical(|ui| {
                ui.label(theme::bold(format!("{} apps", self.apps.len()), 28.0));
                ui.label(theme::dim(format!("{} including their support files", fmt_size(total)), 13.5));
            });
            ui.with_layout(Layout::right_to_left(egui::Align::Center), |ui| {
                let busy = self.scan.is_some();
                let label = if busy {
                    "Refreshing…".to_string()
                } else {
                    format!("{}  Refresh", icon::ARROW_CLOCKWISE)
                };
                if theme::pill(ui, label, Color32::from_white_alpha(35)).clicked() && !busy {
                    self.rescan(ctx);
                }
                for (label, size) in [("Size", true), ("Name", false)] {
                    let fill = if self.sort_size == size {
                        Color32::from_white_alpha(70)
                    } else {
                        Color32::from_white_alpha(18)
                    };
                    if theme::pill(ui, label, fill).clicked() {
                        self.sort_size = size;
                    }
                }
                ui.label(theme::dim("Sort", 12.5));
                ui.add(
                    egui::TextEdit::singleline(&mut self.query)
                        .hint_text("Search apps")
                        .desired_width(180.0)
                        .margin(egui::vec2(10.0, 6.0)),
                );
            });
        });
    }

    fn bottom(&mut self, ui: &mut Ui, p: Palette, ctx: &egui::Context) {
        egui::Panel::bottom("apps_action").frame(egui::Frame::new()).show(ui, |ui| {
            theme::centered(ui, |ui| {
                ui.add_space(4.0);
                let chosen: Vec<AppInfo> = self.apps.iter().filter(|a| a.checked).cloned().collect();
                let size: u64 = chosen.iter().map(AppInfo::total).sum();
                if theme::round_button(ui, "Uninstall", p, !chosen.is_empty()).clicked() {
                    self.remove = Some(Job::spawn(ctx, move |_, _| uninstall(chosen.clone())));
                    return;
                }
                let n = self.apps.iter().filter(|a| a.checked).count();
                let msg = if n == 0 {
                    "Tick the apps you want to remove".to_string()
                } else {
                    format!("{n} selected · {} · moved to Trash", fmt_size(size))
                };
                ui.label(theme::dim(msg, 12.5));
            });
        });
    }

    fn list(&mut self, ui: &mut Ui, ctx: &egui::Context) {
        let q = self.query.to_lowercase();
        let mut order: Vec<usize> = (0..self.apps.len())
            .filter(|&i| q.is_empty() || self.apps[i].name.to_lowercase().contains(&q))
            .collect();
        if self.sort_size {
            order.sort_by(|&a, &b| self.apps[b].total().cmp(&self.apps[a].total()));
        }

        egui::Panel::left("apps_list")
            .resizable(false)
            .exact_size(400.0)
            .frame(egui::Frame::new())
            .show(ui, |ui| {
                let row_h = 54.0;
                egui::ScrollArea::vertical()
                    .auto_shrink(false)
                    .show_rows(ui, row_h, order.len(), |ui, range| {
                        for &i in &order[range] {
                            let focused = self.focus.as_ref() == Some(&self.apps[i].path);
                            let fill = if focused {
                                Color32::from_white_alpha(40)
                            } else if self.apps[i].checked {
                                Color32::from_white_alpha(22)
                            } else {
                                Color32::TRANSPARENT
                            };
                            let resp = egui::Frame::new()
                                .fill(fill)
                                .corner_radius(12)
                                .inner_margin(egui::Margin::symmetric(8, 5))
                                .show(ui, |ui| {
                                    ui.set_width(ui.available_width());
                                    ui.horizontal(|ui| {
                                        ui.checkbox(&mut self.apps[i].checked, "");
                                        let app = self.apps[i].clone();
                                        self.app_icon(ui, &app, 38.0);
                                        ui.with_layout(Layout::right_to_left(egui::Align::Center), |ui| {
                                            ui.label(theme::semi(fmt_size(app.total()), 13.5));
                                            if app.arch == Arch::Intel {
                                                ui.label(theme::ic(icon::WARNING, 14.0).color(theme::WARN))
                                                    .on_hover_text("Intel-only app (runs through Rosetta)");
                                            }
                                            ui.with_layout(Layout::top_down(egui::Align::Min), |ui| {
                                                ui.add_space(2.0);
                                                ui.add(egui::Label::new(theme::semi(&app.name, 14.0)).truncate());
                                                ui.label(theme::dim(app.version.clone().unwrap_or_default(), 11.5));
                                            });
                                        });
                                    });
                                });
                            let click = resp.response.interact(egui::Sense::click());
                            if click.clicked() {
                                let path = self.apps[i].path.clone();
                                if !self.last_opened.contains_key(&path) && self.opened_job.is_none() {
                                    let p2 = path.clone();
                                    self.opened_job = Some(Job::spawn(ctx, move |_, _| (p2.clone(), last_opened(&p2))));
                                }
                                self.focus = Some(path);
                            }
                        }
                    });
            });
    }

    fn detail(&mut self, ui: &mut Ui, ctx: &egui::Context) {
        let Some(i) = self.focus.as_ref().and_then(|f| self.apps.iter().position(|a| &a.path == f)) else {
            theme::card(ui, |ui| {
                ui.set_min_size(ui.available_size());
                theme::centered(ui, |ui| {
                    ui.add_space(ui.available_height() * 0.3);
                    ui.label(theme::ic(icon::PACKAGE, 54.0));
                    ui.label(theme::semi("Select an app", 18.0));
                    ui.label(theme::dim("See its details and every file it keeps on your Mac.", 13.5));
                });
            });
            return;
        };
        // Fetch "last opened" lazily for the focused app.
        let path = self.apps[i].path.clone();
        if !self.last_opened.contains_key(&path) && self.opened_job.is_none() {
            let p2 = path.clone();
            self.opened_job = Some(Job::spawn(ctx, move |_, _| (p2.clone(), last_opened(&p2))));
        }
        let app = self.apps[i].clone();
        let opened = self.last_opened.get(&path).cloned().unwrap_or_else(|| "…".into());

        theme::card(ui, |ui| {
            ui.set_min_size(ui.available_size());
            ui.horizontal(|ui| {
                self.app_icon(ui, &app, 72.0);
                ui.vertical(|ui| {
                    ui.label(theme::bold(&app.name, 24.0));
                    ui.label(theme::dim(
                        format!("Version {}", app.version.as_deref().unwrap_or("unknown")),
                        13.0,
                    ));
                    ui.horizontal(|ui| {
                        if app.app_store {
                            theme::badge(ui, "App Store", Color32::from_rgb(0x5a, 0xa8, 0xff));
                        }
                        arch_badge(ui, app.arch);
                    });
                });
            });
            ui.add_space(8.0);
            egui::Grid::new("app_info")
                .num_columns(4)
                .spacing([16.0, 6.0])
                .min_col_width(60.0)
                .show(ui, |ui| {
                    let cell = |ui: &mut Ui, k: &str, v: String| {
                        ui.label(theme::dim(k, 12.5));
                        ui.label(theme::text(v, 13.0));
                    };
                    cell(ui, "App", fmt_size(app.size));
                    cell(ui, "Last opened", opened);
                    ui.end_row();
                    cell(
                        ui,
                        "Support files",
                        format!(
                            "{} · {} items",
                            fmt_size(app.leftovers.iter().map(|l| l.size).sum()),
                            app.leftovers.len()
                        ),
                    );
                    cell(ui, "Bundle ID", app.bundle_id.clone().unwrap_or_else(|| "—".into()));
                    ui.end_row();
                    cell(ui, "Will remove", fmt_size(app.total()));
                    ui.label(theme::dim("Location", 12.5));
                    if ui
                        .link(theme::text(fsutil::pretty(&app.path), 13.0))
                        .on_hover_text("Reveal in Finder")
                        .clicked()
                    {
                        fsutil::reveal(&app.path);
                    }
                    ui.end_row();
                });
            ui.add_space(6.0);
            ui.horizontal(|ui| {
                let chk = &mut self.apps[i].checked;
                let label = if *chk {
                    format!("{}  Selected for removal", icon::CHECK_CIRCLE)
                } else {
                    "Select for removal".to_string()
                };
                let fill = if *chk {
                    theme::DANGER.gamma_multiply(0.7)
                } else {
                    Color32::from_white_alpha(35)
                };
                if theme::pill(ui, label, fill).clicked() {
                    *chk = !*chk;
                }
            });
            ui.separator();
            ui.label(theme::semi(format!("Related files ({})", app.leftovers.len()), 14.0));
            if app.leftovers.is_empty() {
                ui.label(theme::dim("No support files found in ~/Library.", 12.5));
                return;
            }
            egui::ScrollArea::vertical().auto_shrink(false).show(ui, |ui| {
                for l in &mut self.apps[i].leftovers {
                    ui.horizontal(|ui| {
                        ui.checkbox(&mut l.selected, "");
                        ui.add_sized([96.0, 18.0], egui::Label::new(theme::dim(l.kind, 11.5)));
                        ui.with_layout(Layout::right_to_left(egui::Align::Center), |ui| {
                            ui.add_space(6.0);
                            ui.label(theme::dim(fmt_size(l.size), 12.5));
                            ui.with_layout(Layout::left_to_right(egui::Align::Center), |ui| {
                                let r = ui.add(
                                    egui::Label::new(theme::text(fsutil::pretty(&l.path), 12.5))
                                        .truncate()
                                        .sense(egui::Sense::click()),
                                );
                                if r.on_hover_text("Reveal in Finder").clicked() {
                                    fsutil::reveal(&l.path);
                                }
                            });
                        });
                    });
                }
            });
        });
    }
}
