//! App Uninstaller: removes an app plus its leftovers in ~/Library.
use crate::fsutil::{self, fmt_size};
use crate::job::{self, Job};
use eframe::egui::{self, RichText, Ui};
use rayon::prelude::*;
use std::path::{Path, PathBuf};
use std::process::Command;

#[derive(Clone)]
pub struct AppInfo {
    path: PathBuf,
    name: String,
    bundle_name: Option<String>,
    bundle_id: Option<String>,
    size: u64,
}

struct Leftover {
    path: PathBuf,
    size: u64,
    selected: bool,
}

#[derive(Default)]
pub struct Uninstaller {
    apps: Vec<AppInfo>,
    query: String,
    selected: Option<usize>,
    leftovers: Vec<Leftover>,
    scan: Option<Job<Vec<AppInfo>>>,
    find: Option<Job<(PathBuf, Vec<(PathBuf, u64)>)>>,
    remove: Option<Job<String>>,
    confirm: bool,
    status: String,
    loaded: bool,
}

fn read_plist_string(dict: &plist::Dictionary, key: &str) -> Option<String> {
    dict.get(key).and_then(|v| v.as_string()).map(str::to_owned)
}

fn find_apps() -> Vec<AppInfo> {
    let mut bundles = Vec::new();
    for root in [PathBuf::from("/Applications"), fsutil::home().join("Applications")] {
        for c in fsutil::children(&root) {
            if c.extension().is_some_and(|e| e == "app") {
                bundles.push(c);
            } else if c.is_dir() {
                bundles.extend(fsutil::children(&c).into_iter().filter(|p| p.extension().is_some_and(|e| e == "app")));
            }
        }
    }
    let mut apps: Vec<AppInfo> = bundles
        .into_par_iter()
        .filter_map(|path| {
            let info = plist::Value::from_file(path.join("Contents/Info.plist")).ok();
            let dict = info.as_ref().and_then(|v| v.as_dictionary());
            let bundle_id = dict.and_then(|d| read_plist_string(d, "CFBundleIdentifier"));
            if bundle_id.as_deref().is_some_and(|b| b.starts_with("com.apple.")) {
                return None; // never touch Apple's apps
            }
            Some(AppInfo {
                name: path.file_stem()?.to_string_lossy().into_owned(),
                bundle_name: dict.and_then(|d| read_plist_string(d, "CFBundleName")),
                bundle_id,
                size: fsutil::size_of(&path),
                path,
            })
        })
        .collect();
    apps.sort_by_key(|a| a.name.to_lowercase());
    apps
}

/// Support files in ~/Library matching the bundle ID or the app's exact name.
fn find_leftovers(app: &AppInfo) -> Vec<(PathBuf, u64)> {
    const DIRS: [&str; 13] = [
        "Application Support", "Caches", "Preferences", "Preferences/ByHost", "Containers",
        "Group Containers", "Saved Application State", "Logs", "HTTPStorages", "WebKit",
        "Cookies", "LaunchAgents", "Application Scripts",
    ];
    let lib = fsutil::home().join("Library");
    let mut names = vec![app.name.to_lowercase(), app.name.to_lowercase().replace(' ', "")];
    if let Some(b) = &app.bundle_name {
        names.push(b.to_lowercase());
    }
    let bid = app.bundle_id.as_ref().map(|b| b.to_lowercase());

    let hits: Vec<PathBuf> = DIRS
        .iter()
        .flat_map(|d| fsutil::children(&lib.join(d)))
        .filter(|p| {
            let n = p.file_name().map(|n| n.to_string_lossy().to_lowercase()).unwrap_or_default();
            let stem = Path::new(&n).file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
            names.iter().any(|x| *x == n || *x == stem) || bid.as_ref().is_some_and(|b| n.contains(b.as_str()))
        })
        .collect();
    let mut out: Vec<_> = hits.into_par_iter().map(|p| { let s = fsutil::size_of(&p); (p, s) }).collect();
    out.sort_unstable_by(|a, b| b.1.cmp(&a.1));
    out
}

fn osascript(script: &str) -> String {
    Command::new("osascript")
        .args(["-e", script])
        .output()
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
        .unwrap_or_default()
}

fn uninstall(app: AppInfo, extra: Vec<PathBuf>) -> String {
    if let Some(bid) = &app.bundle_id {
        let running = format!("application id \"{bid}\" is running");
        if osascript(&running) == "true" {
            osascript(&format!("tell application id \"{bid}\" to quit"));
            std::thread::sleep(std::time::Duration::from_millis(1500));
            if osascript(&running) == "true" {
                return format!("{} is still running — quit it and try again.", app.name);
            }
        }
    }
    let mut paths = vec![app.path.clone()];
    paths.extend(extra);
    match fsutil::move_to_trash(&paths) {
        Ok(()) => format!("{} and {} related item(s) moved to the Trash.", app.name, paths.len() - 1),
        Err(e) => format!("Couldn't remove everything: {e}"),
    }
}

impl Uninstaller {
    fn rescan(&mut self, ctx: &egui::Context) {
        self.selected = None;
        self.leftovers.clear();
        self.scan = Some(Job::spawn(ctx, |_, _| find_apps()));
    }

    pub fn ui(&mut self, ui: &mut Ui) {
        let ctx = ui.ctx().clone();
        if !self.loaded {
            self.loaded = true;
            self.rescan(&ctx);
        }
        if let Some(apps) = job::poll(&mut self.scan, &ctx) {
            self.apps = apps;
        }
        if let Some((for_path, found)) = job::poll(&mut self.find, &ctx) {
            // Ignore stale results if the user picked another app meanwhile.
            if self.selected.and_then(|i| self.apps.get(i)).is_some_and(|a| a.path == for_path) {
                self.leftovers = found.into_iter().map(|(path, size)| Leftover { path, size, selected: true }).collect();
            }
        }
        if let Some(msg) = job::poll(&mut self.remove, &ctx) {
            self.status = msg;
            self.rescan(&ctx);
        }

        egui::Panel::left("app_list").resizable(true).default_size(320.0).show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.add(egui::TextEdit::singleline(&mut self.query).hint_text("Search apps").desired_width(200.0));
                if ui.add_enabled(self.scan.is_none(), egui::Button::new("Rescan")).clicked() {
                    self.rescan(&ctx);
                }
            });
            ui.separator();
            if self.scan.is_some() && self.apps.is_empty() {
                ui.horizontal(|ui| {
                    ui.spinner();
                    ui.label("Measuring apps…");
                });
                return;
            }
            let q = self.query.to_lowercase();
            egui::ScrollArea::vertical().auto_shrink(false).show(ui, |ui| {
                for (i, app) in self.apps.iter().enumerate() {
                    if !q.is_empty() && !app.name.to_lowercase().contains(&q) {
                        continue;
                    }
                    let picked = self.selected == Some(i);
                    let label = format!("{}   {}", app.name, fmt_size(app.size));
                    if ui.selectable_label(picked, label).clicked() && !picked {
                        self.selected = Some(i);
                        self.leftovers.clear();
                        self.confirm = false;
                        let a = app.clone();
                        self.find = Some(Job::spawn(&ctx, move |_, _| (a.path.clone(), find_leftovers(&a))));
                    }
                }
            });
        });

        let Some(app) = self.selected.and_then(|i| self.apps.get(i)).cloned() else {
            ui.vertical_centered(|ui| {
                ui.add_space(120.0);
                ui.label(RichText::new("Select an app to see what it left around your Mac.").weak());
                ui.label(&self.status);
            });
            return;
        };

        ui.heading(&app.name);
        ui.label(RichText::new(app.bundle_id.as_deref().unwrap_or("no bundle ID")).weak());
        ui.label(RichText::new(fsutil::pretty(&app.path)).weak());
        ui.add_space(8.0);
        ui.label(RichText::new("Related files").strong());

        egui::Panel::bottom("app_bottom").show(ui, |ui| {
            ui.add_space(6.0);
            ui.horizontal(|ui| {
                ui.label(&self.status);
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    let busy = self.find.is_some() || self.remove.is_some();
                    if self.confirm {
                        if ui.button("Cancel").clicked() {
                            self.confirm = false;
                        }
                        if ui.button(RichText::new("Move to Trash").color(egui::Color32::RED)).clicked() {
                            self.confirm = false;
                            let extra: Vec<PathBuf> = self.leftovers.iter().filter(|l| l.selected).map(|l| l.path.clone()).collect();
                            let a = app.clone();
                            self.remove = Some(Job::spawn(&ctx, move |_, _| uninstall(a, extra)));
                        }
                        ui.label(format!("Uninstall {}?", app.name));
                    } else {
                        if ui.add_enabled(!busy, egui::Button::new("Uninstall")).clicked() {
                            self.confirm = true;
                        }
                        let total = app.size + self.leftovers.iter().filter(|l| l.selected).map(|l| l.size).sum::<u64>();
                        ui.label(format!("Total: {}", fmt_size(total)));
                        if busy {
                            ui.spinner();
                        }
                    }
                });
            });
            ui.add_space(4.0);
        });

        egui::ScrollArea::vertical().auto_shrink(false).show(ui, |ui| {
            if self.find.is_none() && self.leftovers.is_empty() {
                ui.label(RichText::new("No leftovers found in ~/Library.").weak());
            }
            for l in &mut self.leftovers {
                ui.horizontal(|ui| {
                    ui.checkbox(&mut l.selected, "");
                    if ui.link(fsutil::pretty(&l.path)).on_hover_text("Reveal in Finder").clicked() {
                        fsutil::reveal(&l.path);
                    }
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        ui.label(RichText::new(fmt_size(l.size)).weak());
                    });
                });
            }
        });
    }
}
