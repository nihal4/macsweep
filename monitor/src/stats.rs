//! Reads CPU, memory and disk numbers straight from the macOS kernel.
//! Every call here takes microseconds: no subprocesses, no file scanning.

/// What the user chose to show, from MacSweep's settings file.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Config {
    pub cpu: bool,
    pub ram: bool,
    pub disk: bool,
    /// Seconds between updates (1 = live).
    pub interval: f64,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            cpu: true,
            ram: true,
            disk: true,
            interval: 1.0,
        }
    }
}

impl Config {
    /// Parses `key=value` lines; anything unknown or broken keeps its default.
    pub fn parse(text: &str) -> Self {
        let mut c = Self::default();
        for line in text.lines() {
            let Some((k, v)) = line.split_once('=') else { continue };
            let on = v.trim() == "1";
            match k.trim() {
                "cpu" => c.cpu = on,
                "ram" => c.ram = on,
                "disk" => c.disk = on,
                "interval" => {
                    if let Ok(n) = v.trim().parse::<f64>() {
                        c.interval = n.clamp(1.0, 60.0);
                    }
                }
                _ => {}
            }
        }
        c
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Pressure {
    Normal,
    Warning,
    Critical,
}

impl Pressure {
    pub fn from_level(level: i32) -> Self {
        match level {
            4 => Self::Critical,
            2 => Self::Warning,
            _ => Self::Normal,
        }
    }
    pub fn label(self) -> &'static str {
        match self {
            Self::Normal => "🟢 Normal",
            Self::Warning => "🟡 High",
            Self::Critical => "🔴 Critical",
        }
    }
}

/// CPU busy share between two tick samples, 0–100.
pub fn cpu_percent(prev: [u64; 4], now: [u64; 4]) -> f64 {
    let d: Vec<u64> = (0..4).map(|i| now[i].saturating_sub(prev[i])).collect();
    // ticks are [user, system, idle, nice]
    let total: u64 = d.iter().sum();
    if total == 0 {
        return 0.0;
    }
    (d[0] + d[1] + d[3]) as f64 * 100.0 / total as f64
}

/// Decimal units, like Finder.
pub fn fmt_bytes(b: u64) -> String {
    let b = b as f64;
    if b >= 1e12 {
        format!("{:.1} TB", b / 1e12)
    } else if b >= 1e9 {
        format!("{:.0} GB", b / 1e9)
    } else {
        format!("{:.0} MB", b / 1e6)
    }
}

/// The menu-bar text, e.g. "CPU 12%  RAM 61%  120 GB".
pub fn title(c: &Config, cpu: f64, ram_used: u64, ram_total: u64, disk_free: Option<u64>, p: Pressure) -> String {
    let mut parts = Vec::new();
    if c.cpu {
        parts.push(format!("CPU {:.0}%", cpu));
    }
    if c.ram {
        let pct = if ram_total > 0 {
            ram_used as f64 * 100.0 / ram_total as f64
        } else {
            0.0
        };
        let mark = if p == Pressure::Normal { "" } else { " ⚠︎" };
        parts.push(format!("RAM {pct:.0}%{mark}"));
    }
    if c.disk {
        if let Some(f) = disk_free {
            parts.push(fmt_bytes(f));
        }
    }
    if parts.is_empty() {
        "MacSweep".into()
    } else {
        parts.join("  ")
    }
}

#[cfg(target_os = "macos")]
pub use mac::*;

#[cfg(target_os = "macos")]
mod mac {
    use std::ffi::CString;

    type MachPort = u32;
    unsafe extern "C" {
        fn mach_host_self() -> MachPort;
        fn host_statistics(host: MachPort, flavor: i32, info: *mut i32, count: *mut u32) -> i32;
        fn host_statistics64(host: MachPort, flavor: i32, info: *mut i32, count: *mut u32) -> i32;
    }
    const HOST_CPU_LOAD_INFO: i32 = 3;
    const HOST_VM_INFO64: i32 = 4;

    /// `struct vm_statistics64` from <mach/vm_statistics.h>.
    #[repr(C, align(8))]
    #[derive(Default)]
    struct VmStats64 {
        free_count: u32,
        active_count: u32,
        inactive_count: u32,
        wire_count: u32,
        zero_fill_count: u64,
        reactivations: u64,
        pageins: u64,
        pageouts: u64,
        faults: u64,
        cow_faults: u64,
        lookups: u64,
        hits: u64,
        purges: u64,
        purgeable_count: u32,
        speculative_count: u32,
        decompressions: u64,
        compressions: u64,
        swapins: u64,
        swapouts: u64,
        compressor_page_count: u32,
        throttled_count: u32,
        external_page_count: u32,
        internal_page_count: u32,
        total_uncompressed_pages_in_compressor: u64,
    }

    pub struct Sampler {
        host: MachPort,
        page: u64,
        pub ram_total: u64,
        prev_cpu: [u64; 4],
    }

    impl Sampler {
        pub fn new() -> Self {
            // One port for the whole run; asking each time would leak rights.
            let host = unsafe { mach_host_self() };
            let page = unsafe { libc::sysconf(libc::_SC_PAGESIZE) }.max(4096) as u64;
            let mut s = Self {
                host,
                page,
                ram_total: sysctl_u64("hw.memsize").unwrap_or(0),
                prev_cpu: [0; 4],
            };
            s.prev_cpu = s.cpu_ticks();
            s
        }

        fn cpu_ticks(&self) -> [u64; 4] {
            let mut t = [0u32; 4];
            let mut count = 4u32;
            let ok = unsafe { host_statistics(self.host, HOST_CPU_LOAD_INFO, t.as_mut_ptr().cast(), &mut count) };
            if ok != 0 {
                return self.prev_cpu;
            }
            t.map(u64::from)
        }

        /// CPU use since the last call, 0–100.
        pub fn cpu(&mut self) -> f64 {
            let now = self.cpu_ticks();
            let pct = super::cpu_percent(self.prev_cpu, now);
            self.prev_cpu = now;
            pct
        }

        /// "Memory Used" the way Activity Monitor counts it:
        /// app memory + wired + compressed.
        pub fn ram_used(&self) -> u64 {
            let mut v = VmStats64::default();
            let mut count = (std::mem::size_of::<VmStats64>() / 4) as u32;
            let ok = unsafe { host_statistics64(self.host, HOST_VM_INFO64, (&mut v as *mut VmStats64).cast(), &mut count) };
            if ok != 0 {
                return 0;
            }
            let app = u64::from(v.internal_page_count.saturating_sub(v.purgeable_count));
            (app + u64::from(v.wire_count) + u64::from(v.compressor_page_count)) * self.page
        }
    }

    pub fn pressure() -> super::Pressure {
        super::Pressure::from_level(sysctl_u64("kern.memorystatus_vm_pressure_level").unwrap_or(1) as i32)
    }

    /// Free space ignoring purgeable space; used if Foundation can't answer.
    pub fn disk_free_plain() -> Option<u64> {
        let mut s: libc::statfs = unsafe { std::mem::zeroed() };
        let root = CString::new("/").ok()?;
        (unsafe { libc::statfs(root.as_ptr(), &mut s) } == 0).then(|| s.f_bavail * u64::from(s.f_bsize))
    }

    /// Our own resident memory, so the menu can show what the monitor costs.
    pub fn own_memory() -> u64 {
        let mut info: libc::proc_taskinfo = unsafe { std::mem::zeroed() };
        let size = std::mem::size_of::<libc::proc_taskinfo>() as i32;
        let n = unsafe {
            libc::proc_pidinfo(
                libc::getpid(),
                libc::PROC_PIDTASKINFO,
                0,
                (&mut info as *mut libc::proc_taskinfo).cast(),
                size,
            )
        };
        if n == size { info.pti_resident_size } else { 0 }
    }

    fn sysctl_u64(name: &str) -> Option<u64> {
        let c = CString::new(name).ok()?;
        let mut buf = [0u8; 8];
        let mut len = buf.len();
        let ok = unsafe { libc::sysctlbyname(c.as_ptr(), buf.as_mut_ptr().cast(), &mut len, std::ptr::null_mut(), 0) };
        match (ok, len) {
            (0, 8) => Some(u64::from_ne_bytes(buf)),
            (0, 4) => Some(u64::from(u32::from_ne_bytes([buf[0], buf[1], buf[2], buf[3]]))),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn settings_file_is_read_safely() {
        let c = Config::parse("cpu=1\nram=0\ndisk=1\ninterval=2\n");
        assert_eq!(
            c,
            Config {
                cpu: true,
                ram: false,
                disk: true,
                interval: 2.0
            }
        );
        assert_eq!(Config::parse(""), Config::default());
        assert_eq!(
            Config::parse("interval=0.01").interval,
            1.0,
            "never faster than once a second"
        );
        assert_eq!(Config::parse("interval=abc").interval, 1.0);
    }

    #[test]
    fn cpu_share_comes_from_tick_differences() {
        // 30 busy ticks (user+system+nice) out of 100 total
        assert_eq!(cpu_percent([100, 50, 800, 0], [120, 55, 870, 5]), 30.0);
        assert_eq!(cpu_percent([1, 1, 1, 1], [1, 1, 1, 1]), 0.0);
    }

    #[test]
    fn menu_bar_text_follows_the_settings() {
        let all = Config::default();
        let t = title(
            &all,
            12.4,
            8_000_000_000,
            16_000_000_000,
            Some(120_000_000_000),
            Pressure::Normal,
        );
        assert_eq!(t, "CPU 12%  RAM 50%  120 GB");
        let t = title(&all, 3.0, 15_000_000_000, 16_000_000_000, None, Pressure::Warning);
        assert_eq!(t, "CPU 3%  RAM 94% ⚠︎");
        let none = Config {
            cpu: false,
            ram: false,
            disk: false,
            interval: 1.0,
        };
        assert_eq!(title(&none, 0.0, 0, 0, None, Pressure::Normal), "MacSweep");
    }
}
