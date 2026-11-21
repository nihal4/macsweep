//! MacSweep Monitor: a menu-bar item showing CPU, memory and disk.
//!
//! Built to cost almost nothing while it runs:
//! - native AppKit only (macOS draws the text; no graphics engine),
//! - numbers read straight from the kernel (no subprocesses),
//! - one timer every few seconds, with tolerance so macOS can batch the
//!   wake-up with its own, and no updates while the screen is off,
//! - disk space, which changes slowly, is read once a minute.
//!
//! It is started by MacSweep (as a login item) only if the user turns it on.

// On other systems only the shared logic is built (for tests).
#![cfg_attr(not(target_os = "macos"), allow(dead_code))]

mod stats;

#[cfg(not(target_os = "macos"))]
fn main() {
    eprintln!("MacSweep Monitor runs on macOS only.");
}

#[cfg(target_os = "macos")]
fn main() {
    mac::run();
}

#[cfg(target_os = "macos")]
mod mac {
    use crate::stats::{self, Config, Sampler};
    use block2::RcBlock;
    use objc2::rc::Retained;
    use objc2::runtime::{AnyObject, NSObject, ProtocolObject};
    use objc2::{AnyThread, MainThreadMarker, MainThreadOnly, define_class, msg_send, sel};
    use objc2_app_kit::{
        NSApplication, NSApplicationActivationPolicy, NSBitmapImageRep, NSCellImagePosition, NSColor, NSDeviceRGBColorSpace,
        NSFont, NSFontAttributeName, NSFontWeightRegular, NSForegroundColorAttributeName, NSGraphicsContext, NSImage, NSMenu,
        NSMenuDelegate, NSMenuItem, NSScreen, NSStatusBar, NSStatusItem, NSStringDrawing, NSVariableStatusItemLength,
        NSWorkspace, NSWorkspaceScreensDidSleepNotification, NSWorkspaceScreensDidWakeNotification,
    };
    use objc2_foundation::{
        NSArray, NSAttributedStringKey, NSDictionary, NSNotification, NSNumber, NSObjectProtocol, NSPoint, NSRunLoop,
        NSRunLoopCommonModes, NSSize, NSString, NSTimer, NSURL, NSURLVolumeAvailableCapacityForImportantUsageKey, NSUserDefaults,
        ns_string,
    };
    use std::cell::{Cell, RefCell};
    use std::ffi::{CString, c_char, c_void};
    use std::path::PathBuf;
    use std::ptr::NonNull;
    use std::rc::Rc;

    pub const LABEL: &str = "com.nihal.macsweep.monitor";
    /// Posted by MacSweep after it saves new settings (Darwin notification).
    pub const RELOAD: &str = "com.nihal.macsweep.monitor.reload";

    unsafe extern "C" {
        static _dispatch_main_q: c_void;
        fn notify_register_dispatch(
            name: *const c_char,
            out_token: *mut i32,
            queue: *const c_void,
            handler: &block2::Block<dyn Fn(i32)>,
        ) -> u32;
    }

    fn home() -> PathBuf {
        std::env::var_os("HOME").map(PathBuf::from).unwrap_or_default()
    }

    fn config() -> Config {
        let p = home().join("Library/Application Support/MacSweep/monitor.conf");
        Config::parse(&std::fs::read_to_string(p).unwrap_or_default())
    }

    /// MacSweep.app, found from where this helper lives inside it:
    /// MacSweep.app/Contents/Library/LoginItems/MacSweep Monitor.app/Contents/MacOS/…
    fn main_app() -> Option<PathBuf> {
        let exe = std::env::current_exe().ok()?;
        exe.ancestors()
            .filter(|p| p.extension().is_some_and(|e| e == "app"))
            .nth(1)
            .map(PathBuf::from)
    }

    /// Free space the way Finder counts it (includes purgeable space).
    fn disk_free() -> Option<u64> {
        let url = NSURL::fileURLWithPath(ns_string!("/"));
        let key = unsafe { NSURLVolumeAvailableCapacityForImportantUsageKey };
        let keys = NSArray::from_slice(&[key]);
        let found = url
            .resourceValuesForKeys_error(&keys)
            .ok()
            .and_then(|d| d.objectForKey(key))
            .and_then(|v| v.downcast::<NSNumber>().ok())
            .map(|n| n.longLongValue())
            .filter(|&n| n > 0)
            .map(|n| n as u64);
        found.or_else(stats::disk_free_plain)
    }

    /// Everything the monitor keeps between updates. Lives on the main thread.
    struct Monitor {
        mtm: MainThreadMarker,
        item: Retained<NSStatusItem>,
        cpu_line: Retained<NSMenuItem>,
        ram_line: Retained<NSMenuItem>,
        pressure_line: Retained<NSMenuItem>,
        disk_line: Retained<NSMenuItem>,
        own_line: Retained<NSMenuItem>,
        sampler: RefCell<Sampler>,
        config: Cell<Config>,
        disk: Cell<Option<u64>>,
        disk_at: Cell<f64>,
        cpu: Cell<f64>,
        title: RefCell<String>,
        timer: RefCell<Option<Retained<NSTimer>>>,
        asleep: Cell<bool>,
        menu_open: Cell<bool>,
        /// Draw the numbers into a ready-made image instead of handing AppKit
        /// text to lay out and render on every change. Turn off with
        /// `defaults write com.nihal.macsweep.monitor DrawAsText -bool YES`.
        as_image: bool,
        /// Font and colour for drawing (black: the image is a template, so
        /// macOS tints it to match the menu bar).
        attrs: Retained<NSDictionary<NSAttributedStringKey, AnyObject>>,
        scale: f64,
    }

    thread_local! {
        static MONITOR: RefCell<Option<Rc<Monitor>>> = const { RefCell::new(None) };
    }

    fn with_monitor(f: impl FnOnce(&Rc<Monitor>)) {
        if let Some(m) = MONITOR.with(|m| m.borrow().clone()) {
            f(&m);
        }
    }

    fn now() -> f64 {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs_f64())
            .unwrap_or(0.0)
    }

    impl Monitor {
        /// Takes a new sample and redraws. Called once per interval.
        fn tick(&self) {
            if self.asleep.get() {
                return;
            }
            self.cpu.set(self.sampler.borrow_mut().cpu());
            // Disk space changes slowly: every 30 s is plenty.
            if now() - self.disk_at.get() > 30.0 {
                self.disk_at.set(now());
                self.disk.set(disk_free());
            }
            self.redraw();
        }

        /// Renders `text` once into a bitmap the size of the text.
        fn render(&self, text: &str) -> Option<Retained<NSImage>> {
            let s = NSString::from_str(text);
            let size = unsafe { s.sizeWithAttributes(Some(&self.attrs)) };
            let (w, h) = (size.width.ceil().max(1.0), size.height.ceil().max(1.0));
            let rep = unsafe {
                NSBitmapImageRep::initWithBitmapDataPlanes_pixelsWide_pixelsHigh_bitsPerSample_samplesPerPixel_hasAlpha_isPlanar_colorSpaceName_bytesPerRow_bitsPerPixel(
                    NSBitmapImageRep::alloc(),
                    std::ptr::null_mut(),
                    (w * self.scale) as isize,
                    (h * self.scale) as isize,
                    8,
                    4,
                    true,
                    false,
                    NSDeviceRGBColorSpace,
                    0,
                    0,
                )
            }?;
            rep.setSize(NSSize::new(w, h));
            let ctx = NSGraphicsContext::graphicsContextWithBitmapImageRep(&rep)?;
            NSGraphicsContext::saveGraphicsState_class();
            NSGraphicsContext::setCurrentContext(Some(&ctx));
            unsafe { s.drawAtPoint_withAttributes(NSPoint::new(0.0, 0.0), Some(&self.attrs)) };
            NSGraphicsContext::restoreGraphicsState_class();
            let img = NSImage::initWithSize(NSImage::alloc(), NSSize::new(w, h));
            img.addRepresentation(&rep);
            img.setTemplate(true);
            Some(img)
        }

        /// Updates the menu-bar text, and the menu only while it's open.
        fn redraw(&self) {
            let s = self.sampler.borrow();
            let (used, total, p) = (s.ram_used(), s.ram_total, stats::pressure());
            let title = stats::title(&self.config.get(), self.cpu.get(), used, total, self.disk.get(), p);
            // Redrawing costs more than sampling: skip it when nothing changed.
            if *self.title.borrow() != title {
                if let Some(button) = self.item.button(self.mtm) {
                    match self.as_image.then(|| self.render(&title)).flatten() {
                        Some(img) => button.setImage(Some(&img)),
                        None => button.setTitle(&NSString::from_str(&title)),
                    }
                }
                *self.title.borrow_mut() = title;
            }
            if self.menu_open.get() {
                let set = |item: &NSMenuItem, t: String| item.setTitle(&NSString::from_str(&t));
                set(&self.cpu_line, format!("CPU: {:.0}%", self.cpu.get()));
                set(
                    &self.ram_line,
                    format!("Memory: {} of {} used", stats::fmt_bytes(used), stats::fmt_bytes(total)),
                );
                set(&self.pressure_line, format!("Memory pressure: {}", p.label()));
                set(
                    &self.disk_line,
                    match self.disk.get() {
                        Some(f) => format!("Disk: {} available", stats::fmt_bytes(f)),
                        None => "Disk: unknown".into(),
                    },
                );
                set(
                    &self.own_line,
                    format!(
                        "This monitor: {:.1} MB · updates every {:.0} s",
                        stats::own_memory() as f64 / 1e6,
                        self.config.get().interval
                    ),
                );
            }
        }

        /// (Re)starts the update timer at the configured interval.
        fn schedule(self: &Rc<Self>) {
            if let Some(old) = self.timer.borrow_mut().take() {
                old.invalidate();
            }
            let interval = self.config.get().interval;
            let me = Rc::downgrade(self);
            let tick = RcBlock::new(move |_: NonNull<NSTimer>| {
                if let Some(m) = me.upgrade() {
                    m.tick();
                }
            });
            let timer = unsafe { NSTimer::timerWithTimeInterval_repeats_block(interval, true, &tick) };
            // A little slack lets macOS batch our wake-up with its own.
            timer.setTolerance(interval * 0.1);
            // "Common modes" keeps it running while the menu is open, too.
            unsafe { NSRunLoop::mainRunLoop().addTimer_forMode(&timer, NSRunLoopCommonModes) };
            *self.timer.borrow_mut() = Some(timer);
        }

        /// Gives the item a fixed width, wide enough for the largest numbers.
        ///
        /// Profiling showed most of the monitor's CPU time is macOS redrawing
        /// the menu-bar item after each change. With a variable width, every
        /// change also resized the item and re-ran the menu bar's layout;
        /// with a fixed width only the text is redrawn.
        fn fix_width(&self) {
            let Some(button) = self.item.button(self.mtm) else { return };
            let widest = stats::title(
                &self.config.get(),
                100.0,
                100,
                100,
                Some(999_000_000_000),
                stats::Pressure::Warning,
            );
            if self.as_image {
                let w = unsafe { NSString::from_str(&widest).sizeWithAttributes(Some(&self.attrs)) }.width;
                self.item.setLength(w.ceil() + 10.0);
            } else {
                button.setTitle(&NSString::from_str(&widest));
                let w = button.fittingSize().width;
                self.item.setLength(w.ceil() + 4.0);
            }
            self.title.borrow_mut().clear(); // force the real text on the next redraw
        }

        /// New settings from MacSweep: apply at once, no restart.
        fn reload(self: &Rc<Self>) {
            let old = self.config.get();
            let new = config();
            self.config.set(new);
            if new.interval != old.interval {
                self.schedule();
            }
            self.fix_width();
            self.redraw();
        }
    }

    define_class!(
        // Receives the menu's clicks and open/close events.
        #[unsafe(super(NSObject))]
        #[thread_kind = MainThreadOnly]
        #[name = "MacSweepMonitorActions"]
        struct Actions;

        unsafe impl NSObjectProtocol for Actions {}

        unsafe impl NSMenuDelegate for Actions {
            #[unsafe(method(menuWillOpen:))]
            fn menu_will_open(&self, _menu: &NSMenu) {
                with_monitor(|m| {
                    m.menu_open.set(true);
                    m.redraw();
                });
            }

            #[unsafe(method(menuDidClose:))]
            fn menu_did_close(&self, _menu: &NSMenu) {
                with_monitor(|m| m.menu_open.set(false));
            }
        }

        impl Actions {
            #[unsafe(method(openMacSweep:))]
            fn open_macsweep(&self, _sender: Option<&AnyObject>) {
                if let Some(app) = main_app() {
                    let _ = std::process::Command::new("open").arg(app).spawn();
                }
            }

            /// Removes the login item and quits. MacSweep can turn it back on.
            #[unsafe(method(turnOff:))]
            fn turn_off(&self, _sender: Option<&AnyObject>) {
                let plist = home().join(format!("Library/LaunchAgents/{LABEL}.plist"));
                let _ = std::fs::remove_file(&plist);
                // Tell launchd to forget us, without waiting for the answer.
                let _ = std::process::Command::new("launchctl")
                    .args(["remove", LABEL])
                    .spawn();
                std::process::exit(0);
            }
        }
    );

    impl Actions {
        fn new(mtm: MainThreadMarker) -> Retained<Self> {
            unsafe { msg_send![Self::alloc(mtm), init] }
        }
    }

    fn info_item(mtm: MainThreadMarker, menu: &NSMenu) -> Retained<NSMenuItem> {
        let item = NSMenuItem::new(mtm);
        item.setEnabled(false);
        item.setTitle(ns_string!("…"));
        menu.addItem(&item);
        item
    }

    fn action_item(mtm: MainThreadMarker, menu: &NSMenu, title: &str, action: objc2::runtime::Sel, target: &Actions) {
        let item = unsafe {
            NSMenuItem::initWithTitle_action_keyEquivalent(
                NSMenuItem::alloc(mtm),
                &NSString::from_str(title),
                Some(action),
                ns_string!(""),
            )
        };
        unsafe { item.setTarget(Some(target)) };
        menu.addItem(&item);
    }

    pub fn run() {
        let mtm = MainThreadMarker::new().expect("must start on the main thread");
        let app = NSApplication::sharedApplication(mtm);
        // No Dock icon, no app menu: just the menu-bar item.
        app.setActivationPolicy(NSApplicationActivationPolicy::Accessory);

        // The real width is set by fix_width() once the font is in place.
        let item = NSStatusBar::systemStatusBar().statusItemWithLength(NSVariableStatusItemLength);
        // Digits of equal width, so the item doesn't wobble as numbers change.
        let font = NSFont::monospacedDigitSystemFontOfSize_weight(12.0, unsafe { NSFontWeightRegular });
        let as_image = !NSUserDefaults::standardUserDefaults().boolForKey(ns_string!("DrawAsText"));
        if let Some(button) = item.button(mtm) {
            button.setFont(Some(&font));
            if as_image {
                button.setImagePosition(NSCellImagePosition::ImageOnly);
            }
        }
        let black = NSColor::blackColor();
        let font_obj: &AnyObject = font.as_ref();
        let black_obj: &AnyObject = black.as_ref();
        let attrs = NSDictionary::from_slices(
            &[unsafe { NSFontAttributeName }, unsafe { NSForegroundColorAttributeName }],
            &[font_obj, black_obj],
        );
        let scale = NSScreen::mainScreen(mtm).map_or(2.0, |s| s.backingScaleFactor()).max(1.0);

        let actions = Actions::new(mtm);
        let menu = NSMenu::new(mtm);
        menu.setAutoenablesItems(false);
        menu.setDelegate(Some(ProtocolObject::from_ref(&*actions)));
        let cpu_line = info_item(mtm, &menu);
        let ram_line = info_item(mtm, &menu);
        let pressure_line = info_item(mtm, &menu);
        let disk_line = info_item(mtm, &menu);
        menu.addItem(&NSMenuItem::separatorItem(mtm));
        let own_line = info_item(mtm, &menu);
        menu.addItem(&NSMenuItem::separatorItem(mtm));
        action_item(mtm, &menu, "Open MacSweep", sel!(openMacSweep:), &actions);
        action_item(mtm, &menu, "Turn Off Menu Bar Monitor", sel!(turnOff:), &actions);
        item.setMenu(Some(&menu));

        let m = Rc::new(Monitor {
            mtm,
            item,
            cpu_line,
            ram_line,
            pressure_line,
            disk_line,
            own_line,
            sampler: RefCell::new(Sampler::new()),
            config: Cell::new(config()),
            disk: Cell::new(disk_free()),
            disk_at: Cell::new(now()),
            cpu: Cell::new(0.0),
            title: RefCell::new(String::new()),
            timer: RefCell::new(None),
            asleep: Cell::new(false),
            menu_open: Cell::new(false),
            as_image,
            attrs,
            scale,
        });
        MONITOR.with(|g| *g.borrow_mut() = Some(m.clone()));
        m.fix_width();
        m.redraw();
        m.schedule();

        // Settings changed in MacSweep: apply instantly. Costs nothing until it fires.
        let reload = RcBlock::new(|_token: i32| with_monitor(|m| m.reload()));
        let name = CString::new(RELOAD).unwrap();
        let mut token = 0;
        unsafe { notify_register_dispatch(name.as_ptr(), &mut token, &_dispatch_main_q, &reload) };

        // Pause while the displays sleep; catch up the moment they wake.
        let center = NSWorkspace::sharedWorkspace().notificationCenter();
        let sleep = RcBlock::new(|_: NonNull<NSNotification>| with_monitor(|m| m.asleep.set(true)));
        let wake = RcBlock::new(|_: NonNull<NSNotification>| {
            with_monitor(|m| {
                m.asleep.set(false);
                m.disk_at.set(0.0);
                m.tick();
            })
        });
        let _observers = unsafe {
            (
                center.addObserverForName_object_queue_usingBlock(
                    Some(NSWorkspaceScreensDidSleepNotification),
                    None,
                    None,
                    &sleep,
                ),
                center.addObserverForName_object_queue_usingBlock(Some(NSWorkspaceScreensDidWakeNotification), None, None, &wake),
            )
        };

        app.run();
    }
}
