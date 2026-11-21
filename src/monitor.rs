//! Menu Bar Monitor: turns the tiny helper app on or off and sets what it shows.
//!
//! The helper ("MacSweep Monitor.app", inside MacSweep.app) runs on its own as
//! a login item, so MacSweep itself never has to stay open. It is off until
//! the user turns it on here, and turning it off removes it completely.
use crate::fsutil;
use crate::job::{self, Job};
use crate::theme::{self, Palette, icon};
use eframe::egui::{self, Color32, Ui};
use std::path::PathBuf;
use std::process::Command;

pub const LABEL: &str = "com.nihal.macsweep.monitor";

/// What the menu bar shows. Saved as simple `key=value` lines the helper reads.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Config {
    pub cpu: bool,
    pub ram: bool,
    pub disk: bool,
    pub interval: u32,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            cpu: true,
            ram: true,
            disk: true,
            interval: 1,
        }
    }
}

impl Config {
    fn parse(text: &str) -> Self {
        let mut c = Self::default();
        for (k, v) in text.lines().filter_map(|l| l.split_once('=')) {
            let on = v.trim() == "1";
            match k.trim() {
                "cpu" => c.cpu = on,
                "ram" => c.ram = on,
                "disk" => c.disk = on,
                "interval" => c.interval = v.trim().parse().unwrap_or(1).clamp(1, 60),
                _ => {}
            }
        }
        c
    }

    fn render(&self) -> String {
        let b = |x: bool| if x { 1 } else { 0 };
        format!(
            "cpu={}\nram={}\ndisk={}\ninterval={}\n",
            b(self.cpu),
            b(self.ram),
            b(self.disk),
            self.interval
        )
    }

    /// What the menu bar will look like (with example numbers).
    fn preview(&self) -> String {
        let mut v = Vec::new();
        if self.cpu {
            v.push("CPU 12%");
        }
        if self.ram {
            v.push("RAM 61%");
        }
        if self.disk {
            v.push("120 GB");
        }
        if v.is_empty() { "MacSweep".into() } else { v.join("  ") }
    }
}

fn conf_path() -> PathBuf {
    fsutil::home().join("Library/Application Support/MacSweep/monitor.conf")
}

fn plist_path() -> PathBuf {
    fsutil::home().join(format!("Library/LaunchAgents/{LABEL}.plist"))
}

/// The helper's executable inside this copy of MacSweep.app.
fn helper_exe() -> Option<PathBuf> {
    let contents = std::env::current_exe().ok()?.parent()?.parent()?.to_path_buf();
    let exe = contents.join("Library/LoginItems/MacSweep Monitor.app/Contents/MacOS/macsweep-monitor");
    exe.exists().then_some(exe)
}

fn uid() -> u32 {
    unsafe { libc::getuid() }
}

fn launchctl(args: &[&str]) -> bool {
    Command::new("launchctl")
        .args(args)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .is_ok_and(|s| s.success())
}

pub fn is_on() -> bool {
    plist_path().exists()
}

/// The login item that starts the helper at login (and now).
fn login_item(exe: &std::path::Path) -> plist::Value {
    let mut d = plist::Dictionary::new();
    d.insert("Label".into(), LABEL.into());
    d.insert(
        "ProgramArguments".into(),
        plist::Value::Array(vec![exe.to_string_lossy().into_owned().into()]),
    );
    d.insert("RunAtLoad".into(), true.into());
    // Start again only if it crashed, never after "Turn Off".
    let mut keep = plist::Dictionary::new();
    keep.insert("SuccessfulExit".into(), false.into());
    d.insert("KeepAlive".into(), plist::Value::Dictionary(keep));
    d.insert("LimitLoadToSessionType".into(), "Aqua".into());
    plist::Value::Dictionary(d)
}

fn turn_on() -> Result<(), String> {
    let exe = helper_exe().ok_or("The monitor isn't included in this copy of MacSweep. Install the latest release.")?;
    let path = plist_path();
    if let Some(dir) = path.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    login_item(&exe)
        .to_file_xml(&path)
        .map_err(|e| format!("Couldn't save the login item: {e}"))?;
    let domain = format!("gui/{}", uid());
    let _ = launchctl(&["bootout", &format!("{domain}/{LABEL}")]);
    if launchctl(&["bootstrap", &domain, &path.to_string_lossy()]) {
        Ok(())
    } else {
        let _ = std::fs::remove_file(&path);
        Err("macOS didn't start the monitor. Try again, or check System Settings → General → Login Items.".into())
    }
}

fn turn_off() {
    let _ = launchctl(&["bootout", &format!("gui/{}/{LABEL}", uid())]);
    let _ = std::fs::remove_file(plist_path());
}

/// Tells a running monitor to re-read its settings. Instant, never blocks:
/// a Darwin notification, which the monitor receives at no cost while idle.
fn notify_monitor() {
    #[cfg(target_os = "macos")]
    {
        unsafe extern "C" {
            fn notify_post(name: *const std::ffi::c_char) -> u32;
        }
        let name = std::ffi::CString::new("com.nihal.macsweep.monitor.reload").unwrap();
        unsafe { notify_post(name.as_ptr()) };
    }
}

/// If MacSweep was moved or updated, point the login item at this copy.
pub fn repair_on_launch() {
    if !is_on() {
        return;
    }
    let Some(exe) = helper_exe() else { return };
    let current = plist::Value::from_file(plist_path()).ok().and_then(|v| {
        v.as_dictionary()?
            .get("ProgramArguments")?
            .as_array()?
            .first()?
            .as_string()
            .map(PathBuf::from)
    });
    if current.as_deref() != Some(exe.as_path()) {
        let _ = turn_on();
    }
}

/// (memory bytes, CPU %) of the running helper, measured from outside.
fn usage() -> Option<(u64, f32)> {
    let pid = String::from_utf8(Command::new("pgrep").args(["-x", "macsweep-monitor"]).output().ok()?.stdout).ok()?;
    let pid = pid.lines().next()?.trim().to_string();
    let out = String::from_utf8(
        Command::new("ps")
            .args(["-o", "rss=,%cpu=", "-p", &pid])
            .output()
            .ok()?
            .stdout,
    )
    .ok()?;
    let mut it = out.split_whitespace();
    let rss_kb: u64 = it.next()?.parse().ok()?;
    let cpu: f32 = it.next()?.parse().ok()?;
    Some((rss_kb * 1024, cpu))
}

pub struct Monitor {
    config: Config,
    on: bool,
    /// Turning on/off talks to launchd, which can take a moment: off the UI thread.
    switching: Option<Job<Result<bool, String>>>,
    usage: Option<(u64, f32)>,
    usage_job: Option<Job<Option<(u64, f32)>>>,
    usage_at: f64,
    error: Option<String>,
}

impl Default for Monitor {
    fn default() -> Self {
        Self {
            config: Config::parse(&std::fs::read_to_string(conf_path()).unwrap_or_default()),
            on: is_on(),
            switching: None,
            usage: None,
            usage_job: None,
            usage_at: -100.0,
            error: None,
        }
    }
}

impl Monitor {
    fn save(&mut self) {
        let p = conf_path();
        if let Some(dir) = p.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        let _ = std::fs::write(&p, self.config.render());
        if self.on {
            notify_monitor();
        }
    }

    pub fn ui(&mut self, ui: &mut Ui, p: Palette) {
        let ctx = ui.ctx().clone();
        let now = ctx.input(|i| i.time);
        // Measure the helper every few seconds, only while this screen is open.
        if self.on && self.usage_job.is_none() && now - self.usage_at > 3.0 {
            self.usage_at = now;
            self.usage_job = Some(Job::spawn(&ctx, |_, _| usage()));
        }
        if let Some(u) = job::poll(&mut self.usage_job, &ctx) {
            self.usage = u;
        }
        if let Some(r) = job::poll(&mut self.switching, &ctx) {
            match r {
                Ok(on) => {
                    self.on = on;
                    self.usage = None;
                    self.usage_at = now - 2.0; // measure soon after it starts
                }
                Err(e) => {
                    self.on = is_on();
                    self.error = Some(e);
                }
            }
        }
        if self.on {
            ctx.request_repaint_after(std::time::Duration::from_secs(3));
        }

        ui.horizontal(|ui| {
            theme::icon_tile(ui, icon::GAUGE, p.accent, 46.0);
            ui.vertical(|ui| {
                ui.label(theme::bold("Menu Bar Monitor", 28.0));
                let sub = match (self.on, self.usage) {
                    (false, _) => "Off · nothing runs in the background".to_string(),
                    (true, Some((mem, cpu))) => {
                        format!("On · using {:.1} MB of memory and {cpu:.1}% CPU", mem as f64 / 1e6)
                    }
                    (true, None) => "On · starting…".to_string(),
                };
                ui.label(theme::dim(sub, 13.5));
            });
        });
        ui.add_space(10.0);

        egui::Panel::bottom("monitor_action")
            .frame(theme::bottom_frame())
            .show_separator_line(false)
            .show(ui, |ui| {
                theme::centered(ui, |ui| {
                    ui.add_space(4.0);
                    let busy = self.switching.is_some();
                    let label = match (busy, self.on) {
                        (true, true) => "Turning off…",
                        (true, false) => "Turning on…",
                        (false, true) => "Turn Off",
                        (false, false) => "Turn On",
                    };
                    if theme::round_button(ui, label, p, !busy).clicked() {
                        self.error = None;
                        let turning_on = !self.on;
                        if turning_on {
                            self.save();
                        }
                        self.switching = Some(Job::spawn(&ctx, move |_, _| {
                            if turning_on {
                                turn_on().map(|_| true)
                            } else {
                                turn_off();
                                Ok(false)
                            }
                        }));
                    }
                    let msg = match (&self.error, self.on) {
                        (Some(e), _) => e.clone(),
                        (None, true) => "Runs on its own and starts at login. MacSweep can be closed.".into(),
                        (None, false) => "Off by default. Turning it off removes it completely.".into(),
                    };
                    ui.label(theme::dim(msg, 12.5));
                });
            });

        egui::ScrollArea::vertical().auto_shrink(false).show(ui, |ui| {
            theme::card(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.label(theme::semi("What to show", 15.0));
                ui.add_space(6.0);
                // A small mock of the menu bar, so the choice is easy to judge.
                egui::Frame::new()
                    .fill(Color32::from_black_alpha(90))
                    .corner_radius(6)
                    .inner_margin(egui::Margin::symmetric(12, 5))
                    .show(ui, |ui| {
                        ui.label(theme::text(self.config.preview(), 13.0).monospace());
                    });
                ui.add_space(8.0);
                let mut changed = false;
                ui.horizontal(|ui| {
                    changed |= ui.checkbox(&mut self.config.cpu, "CPU").changed();
                    changed |= ui.checkbox(&mut self.config.ram, "Memory").changed();
                    changed |= ui.checkbox(&mut self.config.disk, "Free disk space").changed();
                });
                ui.add_space(8.0);
                ui.horizontal(|ui| {
                    ui.label(theme::dim("Update every", 13.0));
                    for (s, name) in [(1u32, "Live"), (2, "2 s"), (5, "5 s")] {
                        let fill = if self.config.interval == s {
                            Color32::from_white_alpha(70)
                        } else {
                            Color32::from_white_alpha(18)
                        };
                        if theme::pill(ui, name, fill).clicked() && self.config.interval != s {
                            self.config.interval = s;
                            changed = true;
                        }
                    }
                    ui.label(theme::dim("Live updates every second. Changes apply instantly.", 12.0));
                });
                if changed {
                    self.save();
                }
            });
            ui.add_space(10.0);
            theme::card(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.label(theme::semi("Built to stay out of your way", 15.0));
                ui.add_space(6.0);
                for (glyph, line) in [
                    (
                        icon::CPU,
                        "A separate, tiny native helper. MacSweep itself doesn't stay open.",
                    ),
                    (
                        icon::PULSE,
                        "Numbers come straight from macOS: no extra programs, no scanning.",
                    ),
                    (
                        icon::POWER,
                        "Redraws only when a number changes, and pauses while the screen is off.",
                    ),
                    (
                        icon::LOCK_SIMPLE,
                        "Off until you turn it on. Turning it off removes it completely.",
                    ),
                ] {
                    ui.horizontal(|ui| {
                        ui.label(theme::text(glyph, 14.0).color(p.accent));
                        ui.label(theme::text(line, 13.0));
                    });
                }
                if let (true, Some((mem, cpu))) = (self.on, self.usage) {
                    ui.add_space(6.0);
                    ui.label(theme::dim(
                        format!(
                            "Right now: {:.1} MB of memory, {cpu:.1}% CPU. The menu also shows what it uses.",
                            mem as f64 / 1e6
                        ),
                        12.5,
                    ));
                }
            });
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn settings_round_trip_and_preview() {
        let c = Config {
            cpu: false,
            ram: true,
            disk: true,
            interval: 10,
        };
        assert_eq!(Config::parse(&c.render()), c);
        assert_eq!(c.preview(), "RAM 61%  120 GB");
        assert_eq!(Config::parse("").preview(), "CPU 12%  RAM 61%  120 GB");
    }

    #[test]
    fn login_item_restarts_only_after_a_crash() {
        let v = login_item(std::path::Path::new("/Applications/MacSweep.app/x"));
        let d = v.as_dictionary().unwrap();
        assert_eq!(d.get("Label").and_then(|v| v.as_string()), Some(LABEL));
        assert_eq!(d.get("RunAtLoad").and_then(|v| v.as_boolean()), Some(true));
        let keep = d.get("KeepAlive").and_then(|v| v.as_dictionary()).unwrap();
        assert_eq!(keep.get("SuccessfulExit").and_then(|v| v.as_boolean()), Some(false));
    }
}
